#pragma once
#include <cstdint>
#include <string>
#include <vector>
#include <functional>
#include <memory>
#include <thread>
#include <atomic>
#include <mutex>
#include <queue>
#include <unordered_map>

namespace devops {

struct IPCConfig {
    std::string sandbox_id;
    std::string socket_path = "/tmp/sandbox-ipc";
    uint32_t max_message_size = 64 * 1024;  // 64 KB
    uint32_t queue_size = 1000;
    bool enable_logging = false;
};

struct IPCMessage {
    std::string from;
    std::string to;      // empty = broadcast
    std::string topic;
    std::vector<uint8_t> payload;
    uint64_t timestamp;
    uint64_t sequence;

    // Convenience constructors
    static IPCMessage create(const std::string& from, const std::string& to,
                            const std::string& topic, const std::string& data) {
        IPCMessage msg;
        msg.from = from;
        msg.to = to;
        msg.topic = topic;
        msg.payload.assign(data.begin(), data.end());
        msg.timestamp = std::chrono::duration_cast<std::chrono::milliseconds>(
            std::chrono::steady_clock::now().time_since_epoch()).count();
        return msg;
    }

    std::string as_string() const {
        return std::string(payload.begin(), payload.end());
    }
};

class IPCChannel {
public:
    IPCChannel(const IPCConfig& config);
    ~IPCChannel();

    // Connect to IPC network
    bool connect();
    bool disconnect();
    bool is_connected() const;

    // Send messages
    bool send(const std::string& to, const std::string& topic,
             const std::string& data);
    bool send_raw(const std::string& to, const std::string& topic,
                  const std::vector<uint8_t>& data);
    bool broadcast(const std::string& topic, const std::string& data);

    // Receive messages
    bool subscribe(const std::string& topic,
                  std::function<void(const IPCMessage&)> callback);
    bool unsubscribe(const std::string& topic);

    // Request-reply
    using ReplyCallback = std::function<void(const IPCMessage&)>;
    bool request(const std::string& to, const std::string& topic,
                const std::string& data, ReplyCallback reply_cb,
                uint32_t timeout_ms = 5000);

    // Message queue
    struct QueueStats {
        uint64_t messages_sent;
        uint64_t messages_received;
        uint64_t messages_dropped;
        uint64_t bytes_sent;
        uint64_t bytes_received;
    };
    QueueStats get_stats() const;

    // Get pending messages
    std::vector<IPCMessage> get_pending(uint32_t max_count = 100);

    // Drain queue
    bool drain();

private:
    void receive_loop();
    void handle_message(const IPCMessage& msg);

    IPCConfig config_;
    std::atomic<bool> connected_{false};
    std::thread receive_thread_;

    mutable std::mutex send_mutex_;
    mutable std::mutex recv_mutex_;

    struct PendingRequest {
        ReplyCallback callback;
        uint64_t timestamp;
        uint32_t timeout_ms;
    };

    std::unordered_map<std::string, std::vector<std::function<void(const IPCMessage&)>>> subscribers_;
    std::queue<IPCMessage> message_queue_;
    std::unordered_map<std::string, PendingRequest> pending_requests_;

    std::atomic<uint64_t> sequence_{0};
    std::atomic<uint64_t> messages_sent_{0};
    std::atomic<uint64_t> messages_received_{0};
    std::atomic<uint64_t> messages_dropped_{0};
    std::atomic<uint64_t> bytes_sent_{0};
    std::atomic<uint64_t> bytes_received_{0};
};

// Manages IPC between multiple sandboxes
class IPCManager {
public:
    IPCManager(const IPCConfig& base_config);
    ~IPCManager();

    // Register a sandbox
    std::shared_ptr<IPCChannel> register_sandbox(const std::string& sandbox_id);

    // Unregister a sandbox
    bool unregister_sandbox(const std::string& sandbox_id);

    // Get channel for a sandbox
    std::shared_ptr<IPCChannel> get_channel(const std::string& sandbox_id);

    // Broadcast to all sandboxes
    bool broadcast_all(const std::string& topic, const std::string& data);

    // Get global stats
    struct GlobalStats {
        uint64_t total_channels;
        uint64_t total_messages;
        uint64_t total_bytes;
        uint64_t active_subscriptions;
    };
    GlobalStats get_global_stats() const;

private:
    IPCConfig base_config_;
    std::unordered_map<std::string, std::shared_ptr<IPCChannel>> channels_;
    mutable std::mutex channels_mutex_;
};

} // namespace devops
