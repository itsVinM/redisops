#include <devops/ipc.hpp>
#include <format>
#include <iostream>
#include <fstream>
#include <chrono>
#include <algorithm>
#include <cstring>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>
#include <fcntl.h>
#include <poll.h>

namespace devops {

// ── IPCChannel ──

IPCChannel::IPCChannel(const IPCConfig& config) : config_(config) {}

IPCChannel::~IPCChannel() {
    disconnect();
}

bool IPCChannel::connect() {
    if (connected_) return true;

    // Create Unix domain socket
    int sock = socket(AF_UNIX, SOCK_DGRAM, 0);
    if (sock < 0) {
        std::cerr << std::format("[ipc] failed to create socket: {}\n", strerror(errno));
        return false;
    }

    // Set non-blocking
    int flags = fcntl(sock, F_GETFL, 0);
    fcntl(sock, F_SETFL, flags | O_NONBLOCK);

    // Bind to socket path
    struct sockaddr_un addr;
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;

    std::string socket_path = config_.socket_path + "-" + config_.sandbox_id;
    strncpy(addr.sun_path, socket_path.c_str(), sizeof(addr.sun_path) - 1);

    unlink(socket_path.c_str());  // Remove old socket
    if (bind(sock, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
        std::cerr << std::format("[ipc] failed to bind: {}\n", strerror(errno));
        close(sock);
        return false;
    }

    connected_ = true;

    // Start receive loop
    receive_thread_ = std::thread(&IPCChannel::receive_loop, this);

    std::cout << std::format("[ipc] connected: {}\n", config_.sandbox_id);
    return true;
}

bool IPCChannel::disconnect() {
    if (!connected_) return true;

    connected_ = false;

    if (receive_thread_.joinable()) {
        receive_thread_.join();
    }

    // Remove socket file
    std::string socket_path = config_.socket_path + "-" + config_.sandbox_id;
    unlink(socket_path.c_str());

    std::cout << std::format("[ipc] disconnected: {}\n", config_.sandbox_id);
    return true;
}

bool IPCChannel::is_connected() const {
    return connected_;
}

bool IPCChannel::send(const std::string& to, const std::string& topic,
                      const std::string& data) {
    return send_raw(to, topic, std::vector<uint8_t>(data.begin(), data.end()));
}

bool IPCChannel::send_raw(const std::string& to, const std::string& topic,
                          const std::vector<uint8_t>& data) {
    if (!connected_) return false;

    IPCMessage msg;
    msg.from = config_.sandbox_id;
    msg.to = to;
    msg.topic = topic;
    msg.payload = data;
    msg.timestamp = std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count();
    msg.sequence = sequence_.fetch_add(1);

    // Serialize message
    std::string serialized;
    serialized.reserve(1024 + data.size());
    serialized.append(msg.from);
    serialized.push_back('\0');
    serialized.append(msg.to);
    serialized.push_back('\0');
    serialized.append(msg.topic);
    serialized.push_back('\0');

    // Timestamp
    std::string ts = std::to_string(msg.timestamp);
    serialized.append(ts);
    serialized.push_back('\0');

    // Sequence
    std::string seq = std::to_string(msg.sequence);
    serialized.append(seq);
    serialized.push_back('\0');

    // Payload length
    uint32_t len = data.size();
    serialized.append(reinterpret_cast<const char*>(&len), 4);

    // Payload
    serialized.append(reinterpret_cast<const char*>(data.data()), data.size());

    // Send to target socket
    std::string target_socket = config_.socket_path + "-" + to;

    int sock = socket(AF_UNIX, SOCK_DGRAM, 0);
    if (sock < 0) return false;

    struct sockaddr_un addr;
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    strncpy(addr.sun_path, target_socket.c_str(), sizeof(addr.sun_path) - 1);

    ssize_t sent = sendto(sock, serialized.data(), serialized.size(), 0,
                         (struct sockaddr*)&addr, sizeof(addr));

    close(sock);

    if (sent < 0) {
        messages_dropped_++;
        return false;
    }

    messages_sent_++;
    bytes_sent_ += sent;

    if (config_.enable_logging) {
        std::cout << std::format("[ipc] {} -> {} ({})\n", config_.sandbox_id, to, topic);
    }

    return true;
}

bool IPCChannel::broadcast(const std::string& topic, const std::string& data) {
    // Send to empty string (broadcast)
    return send("", topic, data);
}

bool IPCChannel::subscribe(const std::string& topic,
                          std::function<void(const IPCMessage&)> callback) {
    std::lock_guard<std::mutex> lock(recv_mutex_);
    subscribers_[topic].push_back(std::move(callback));
    return true;
}

bool IPCChannel::unsubscribe(const std::string& topic) {
    std::lock_guard<std::mutex> lock(recv_mutex_);
    subscribers_.erase(topic);
    return true;
}

bool IPCChannel::request(const std::string& to, const std::string& topic,
                        const std::string& data, ReplyCallback reply_cb,
                        uint32_t timeout_ms) {
    std::string reply_topic = topic + ":reply:" + std::to_string(sequence_.load());

    // Subscribe to reply topic
    subscribe(reply_topic, [this, reply_cb](const IPCMessage& msg) {
        reply_cb(msg);
    });

    // Send request
    return send(to, topic, data);
}

void IPCChannel::receive_loop() {
    int sock = socket(AF_UNIX, SOCK_DGRAM, 0);
    if (sock < 0) return;

    struct sockaddr_un addr;
    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;

    std::string socket_path = config_.socket_path + "-" + config_.sandbox_id;
    strncpy(addr.sun_path, socket_path.c_str(), sizeof(addr.sun_path) - 1);

    if (bind(sock, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
        close(sock);
        return;
    }

    uint8_t buffer[65536];

    while (connected_) {
        struct pollfd pfd;
        pfd.fd = sock;
        pfd.events = POLLIN;
        pfd.revents = 0;

        int ret = poll(&pfd, 1, 100);  // 100ms timeout
        if (ret <= 0) continue;

        ssize_t received = recv(sock, buffer, sizeof(buffer), 0);
        if (received <= 0) continue;

        // Deserialize message
        IPCMessage msg;
        size_t pos = 0;

        // Parse fields separated by null bytes
        auto read_field = [&]() -> std::string {
            std::string field;
            while (pos < received && buffer[pos] != '\0') {
                field += static_cast<char>(buffer[pos++]);
            }
            pos++;  // Skip null terminator
            return field;
        };

        msg.from = read_field();
        msg.to = read_field();
        msg.topic = read_field();
        msg.timestamp = std::stoull(read_field());
        msg.sequence = std::stoull(read_field());

        // Payload length
        uint32_t len = 0;
        memcpy(&len, buffer + pos, 4);
        pos += 4;

        // Payload
        msg.payload.assign(buffer + pos, buffer + pos + len);

        messages_received_++;
        bytes_received_ += received;

        handle_message(msg);
    }

    close(sock);
}

void IPCChannel::handle_message(const IPCMessage& msg) {
    std::lock_guard<std::mutex> lock(recv_mutex_);

    // Check if message is for us (or broadcast)
    if (!msg.to.empty() && msg.to != config_.sandbox_id) {
        return;
    }

    // Queue the message
    if (message_queue_.size() < config_.queue_size) {
        message_queue_.push(msg);
    } else {
        messages_dropped_++;
    }

    // Notify subscribers
    auto it = subscribers_.find(msg.topic);
    if (it != subscribers_.end()) {
        for (const auto& callback : it->second) {
            callback(msg);
        }
    }

    // Also notify wildcard subscribers
    it = subscribers_.find("*");
    if (it != subscribers_.end()) {
        for (const auto& callback : it->second) {
            callback(msg);
        }
    }
}

IPCChannel::QueueStats IPCChannel::get_stats() const {
    return {messages_sent_.load(), messages_received_.load(),
            messages_dropped_.load(), bytes_sent_.load(), bytes_received_.load()};
}

std::vector<IPCMessage> IPCChannel::get_pending(uint32_t max_count) {
    std::lock_guard<std::mutex> lock(recv_mutex_);
    std::vector<IPCMessage> result;

    while (!message_queue_.empty() && result.size() < max_count) {
        result.push_back(message_queue_.front());
        message_queue_.pop();
    }

    return result;
}

bool IPCChannel::drain() {
    std::lock_guard<std::mutex> lock(recv_mutex_);
    while (!message_queue_.empty()) {
        message_queue_.pop();
    }
    return true;
}

// ── IPCManager ──

IPCManager::IPCManager(const IPCConfig& base_config) : base_config_(base_config) {}

IPCManager::~IPCManager() {
    for (auto& [id, channel] : channels_) {
        channel->disconnect();
    }
}

std::shared_ptr<IPCChannel> IPCManager::register_sandbox(const std::string& sandbox_id) {
    IPCConfig config = base_config_;
    config.sandbox_id = sandbox_id;

    auto channel = std::make_shared<IPCChannel>(config);
    if (!channel->connect()) {
        return nullptr;
    }

    std::lock_guard<std::mutex> lock(channels_mutex_);
    channels_[sandbox_id] = channel;

    std::cout << std::format("[ipc-manager] registered: {}\n", sandbox_id);
    return channel;
}

bool IPCManager::unregister_sandbox(const std::string& sandbox_id) {
    std::lock_guard<std::mutex> lock(channels_mutex_);
    auto it = channels_.find(sandbox_id);
    if (it == channels_.end()) return false;

    it->second->disconnect();
    channels_.erase(it);

    std::cout << std::format("[ipc-manager] unregistered: {}\n", sandbox_id);
    return true;
}

std::shared_ptr<IPCChannel> IPCManager::get_channel(const std::string& sandbox_id) {
    std::lock_guard<std::mutex> lock(channels_mutex_);
    auto it = channels_.find(sandbox_id);
    return it != channels_.end() ? it->second : nullptr;
}

bool IPCManager::broadcast_all(const std::string& topic, const std::string& data) {
    std::lock_guard<std::mutex> lock(channels_mutex_);
    for (auto& [id, channel] : channels_) {
        channel->broadcast(topic, data);
    }
    return true;
}

IPCManager::GlobalStats IPCManager::get_global_stats() const {
    std::lock_guard<std::mutex> lock(channels_mutex_);
    GlobalStats stats = {};
    stats.total_channels = channels_.size();

    for (const auto& [id, channel] : channels_) {
        auto channel_stats = channel->get_stats();
        stats.total_messages += channel_stats.messages_sent + channel_stats.messages_received;
        stats.total_bytes += channel_stats.bytes_sent + channel_stats.bytes_received;
    }

    return stats;
}

} // namespace devops
