#include <devops/redis_client.hpp>
#include <arpa/inet.h>
#include <netinet/tcp.h>
#include <sys/socket.h>
#include <unistd.h>
#include <cstring>
#include <format>
#include <iostream>

namespace devops {

RedisClient::RedisClient() = default;

RedisClient::~RedisClient() { close(); }

bool RedisClient::connect(const std::string& host, uint16_t port) {
    fd_ = socket(AF_INET, SOCK_STREAM, 0);
    if (fd_ < 0) return false;

    int flag = 1;
    setsockopt(fd_, IPPROTO_TCP, TCP_NODELAY, &flag, sizeof(flag));

    sockaddr_in addr{};
    addr.sin_family = AF_INET;
    addr.sin_port = htons(port);
    inet_pton(AF_INET, host.c_str(), &addr.sin_addr);

    if (::connect(fd_, reinterpret_cast<sockaddr*>(&addr), sizeof(addr)) < 0) {
        close();
        return false;
    }
    return true;
}

void RedisClient::close() {
    if (fd_ >= 0) { ::close(fd_); fd_ = -1; }
}

bool RedisClient::is_connected() const { return fd_ >= 0; }

bool RedisClient::write_all(const uint8_t* data, size_t len) {
    size_t sent = 0;
    while (sent < len) {
        ssize_t n = ::send(fd_, data + sent, len - sent, 0);
        if (n <= 0) return false;
        sent += n;
    }
    return true;
}

bool RedisClient::read_exact(uint8_t* buf, size_t len) {
    size_t got = 0;
    while (got < len) {
        ssize_t n = ::recv(fd_, buf + got, len - got, 0);
        if (n <= 0) return false;
        got += n;
    }
    return true;
}

std::optional<Response> RedisClient::send(const std::vector<std::string>& args) {
    // Encode: [u32 msg_len][u32 n_args][u32 arg_len][arg_bytes]...
    uint32_t payload_size = 4; // n_args
    for (const auto& arg : args) {
        payload_size += 4 + arg.size();
    }

    // Message = [u32 msg_len][payload]
    std::vector<uint8_t> msg(4 + payload_size);
    uint32_t msg_len = payload_size;
    std::memcpy(msg.data(), &msg_len, 4);
    uint32_t n = args.size();
    std::memcpy(msg.data() + 4, &n, 4);

    size_t offset = 8;
    for (const auto& arg : args) {
        uint32_t len = arg.size();
        std::memcpy(msg.data() + offset, &len, 4);
        std::memcpy(msg.data() + offset + 4, arg.data(), arg.size());
        offset += 4 + arg.size();
    }

    if (!write_all(msg.data(), msg.size())) return std::nullopt;
    return read_response();
}

// Helper to parse from buffer (forward declaration)
static std::optional<Response> read_value_from(const std::vector<uint8_t>& buf, size_t& pos);

std::optional<Response> RedisClient::read_response() {
    uint32_t len;
    if (!read_exact(reinterpret_cast<uint8_t*>(&len), 4)) return std::nullopt;
    if (len > 64 * 1024 * 1024) return std::nullopt;

    std::vector<uint8_t> buf(len);
    if (!read_exact(buf.data(), len)) return std::nullopt;

    // Parse value from buffer
    size_t pos = 0;
    return read_value_from(buf, pos);
}

// Helper to parse from buffer
static std::optional<Response> read_value_from(const std::vector<uint8_t>& buf, size_t& pos) {
    if (pos >= buf.size()) return std::nullopt;

    Response resp;
    resp.tag = static_cast<ResponseTag>(buf[pos++]);

    switch (resp.tag) {
        case ResponseTag::Nil:
            return resp;

        case ResponseTag::Error: {
            if (pos + 8 > buf.size()) return std::nullopt;
            std::memcpy(&resp.err_code, buf.data() + pos, 4);
            pos += 4;
            uint32_t msg_len;
            std::memcpy(&msg_len, buf.data() + pos, 4);
            pos += 4;
            if (pos + msg_len > buf.size()) return std::nullopt;
            resp.err_msg.assign(buf.begin() + pos, buf.begin() + pos + msg_len);
            pos += msg_len;
            return resp;
        }

        case ResponseTag::Str: {
            if (pos + 4 > buf.size()) return std::nullopt;
            uint32_t slen;
            std::memcpy(&slen, buf.data() + pos, 4);
            pos += 4;
            if (pos + slen > buf.size()) return std::nullopt;
            resp.str.assign(buf.begin() + pos, buf.begin() + pos + slen);
            pos += slen;
            return resp;
        }

        case ResponseTag::Int: {
            if (pos + 8 > buf.size()) return std::nullopt;
            std::memcpy(&resp.integer, buf.data() + pos, 8);
            pos += 8;
            return resp;
        }

        case ResponseTag::Dbl: {
            if (pos + 8 > buf.size()) return std::nullopt;
            std::memcpy(&resp.dbl, buf.data() + pos, 8);
            pos += 8;
            return resp;
        }

        case ResponseTag::Arr: {
            if (pos + 4 > buf.size()) return std::nullopt;
            uint32_t count;
            std::memcpy(&count, buf.data() + pos, 4);
            pos += 4;
            resp.arr.reserve(count);
            for (uint32_t i = 0; i < count; ++i) {
                auto elem = read_value_from(buf, pos);
                if (!elem) return std::nullopt;
                resp.arr.push_back(std::move(*elem));
            }
            return resp;
        }
    }
    return std::nullopt;
}

// The public read_response needs to call the free function
std::optional<Response> RedisClient::read_value() {
    uint32_t len;
    if (!read_exact(reinterpret_cast<uint8_t*>(&len), 4)) return std::nullopt;
    std::vector<uint8_t> buf(len);
    if (!read_exact(buf.data(), len)) return std::nullopt;
    size_t pos = 0;
    return read_value_from(buf, pos);
}

// ── Convenience wrappers ──

std::optional<Response> RedisClient::set(const std::string& key, const std::string& val) {
    return send({"set", key, val});
}

std::optional<Response> RedisClient::get(const std::string& key) {
    return send({"get", key});
}

std::optional<Response> RedisClient::del(const std::string& key) {
    return send({"del", key});
}

std::optional<Response> RedisClient::lpush(const std::string& key, const std::string& val) {
    return send({"lpush", key, val});
}

std::optional<Response> RedisClient::rpop(const std::string& key) {
    return send({"rpop", key});
}

std::optional<Response> RedisClient::lrange(const std::string& key, int64_t start, int64_t stop) {
    return send({"lrange", key, std::to_string(start), std::to_string(stop)});
}

std::optional<Response> RedisClient::llen(const std::string& key) {
    return send({"llen", key});
}

std::optional<Response> RedisClient::job_next() {
    return send({"job next"});
}

std::optional<Response> RedisClient::job_status(const std::string& id) {
    return send({"job status", id});
}

std::optional<Response> RedisClient::job_result(const std::string& id, int exit_code, int64_t duration_ms) {
    return send({"job result", id, std::to_string(exit_code), std::to_string(duration_ms)});
}

std::optional<Response> RedisClient::job_log(const std::string& id, const std::string& line) {
    return send({"job log", id, line});
}

std::optional<Response> RedisClient::sandbox_register(const std::string& id, const std::string& type, const std::string& addr) {
    return send({"sandbox register", id, type, addr});
}

std::optional<Response> RedisClient::sandbox_claim(const std::string& id, const std::string& job_id) {
    return send({"sandbox claim", id, job_id});
}

std::optional<Response> RedisClient::sandbox_release(const std::string& id) {
    return send({"sandbox release", id});
}

std::optional<Response> RedisClient::metric_record(const std::string& name, double value) {
    return send({"metric record", name, std::format("{:.6}", value)});
}

std::optional<Response> RedisClient::metric_summary() {
    return send({"metric summary"});
}

} // namespace devops
