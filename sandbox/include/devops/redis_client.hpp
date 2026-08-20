#pragma once
#include <cstdint>
#include <string>
#include <vector>
#include <optional>
#include <memory>

namespace devops {

enum class ResponseTag : uint8_t {
    Nil   = 0,
    Error = 1,
    Str   = 2,
    Int   = 3,
    Dbl   = 4,
    Arr   = 5,
};

struct Response {
    ResponseTag tag;
    int32_t err_code = 0;
    std::string err_msg;
    std::string str;
    int64_t integer = 0;
    double dbl = 0.0;
    std::vector<Response> arr;

    bool is_nil() const { return tag == ResponseTag::Nil; }
    bool is_error() const { return tag == ResponseTag::Error; }

    const std::string& as_str() const { return str; }
    int64_t as_int() const { return integer; }
    double as_dbl() const { return dbl; }
    const std::vector<Response>& as_arr() const { return arr; }
};

class RedisClient {
public:
    RedisClient();
    ~RedisClient();

    bool connect(const std::string& host, uint16_t port);
    void close();
    bool is_connected() const;

    std::optional<Response> send(const std::vector<std::string>& args);

    // Convenience methods
    std::optional<Response> set(const std::string& key, const std::string& val);
    std::optional<Response> get(const std::string& key);
    std::optional<Response> del(const std::string& key);
    std::optional<Response> lpush(const std::string& key, const std::string& val);
    std::optional<Response> rpop(const std::string& key);
    std::optional<Response> lrange(const std::string& key, int64_t start, int64_t stop);
    std::optional<Response> llen(const std::string& key);

    // DevOps commands
    std::optional<Response> job_next();
    std::optional<Response> job_status(const std::string& id);
    std::optional<Response> job_result(const std::string& id, int exit_code, int64_t duration_ms);
    std::optional<Response> job_log(const std::string& id, const std::string& line);
    std::optional<Response> sandbox_register(const std::string& id, const std::string& type, const std::string& addr);
    std::optional<Response> sandbox_claim(const std::string& id, const std::string& job_id);
    std::optional<Response> sandbox_release(const std::string& id);
    std::optional<Response> metric_record(const std::string& name, double value);
    std::optional<Response> metric_summary();

private:
    bool write_all(const uint8_t* data, size_t len);
    bool read_exact(uint8_t* buf, size_t len);
    std::optional<Response> read_response();
    std::optional<Response> read_value();

    int fd_ = -1;
};

} // namespace devops
