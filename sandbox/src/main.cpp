#include <devops/redis_client.hpp>
#include <devops/sandbox.hpp>
#include <format>
#include <iostream>
#include <string>
#include <thread>
#include <chrono>
#include <cstring>
#include <cstdlib>
#include <getopt.h>

static void print_usage(const char* prog) {
    std::cerr << std::format(
        "Usage: {} [options]\n"
        "  -h, --host HOST     Redis host (default: 127.0.0.1)\n"
        "  -p, --port PORT     Redis port (default: 1234)\n"
        "  -i, --id ID         Sandbox ID (default: auto)\n"
        "  -t, --type TYPE     Target type (default: local)\n"
        "  -n, --no-seccomp    Disable seccomp filter\n"
        "  --help              Show this help\n",
        prog);
}

int main(int argc, char* argv[]) {
    std::string host = "127.0.0.1";
    uint16_t port = 1234;
    std::string sandbox_id = "cpp-sandbox-1";
    std::string target_type = "local";
    bool use_seccomp = true;

    static struct option long_opts[] = {
        {"host",        required_argument, nullptr, 'h'},
        {"port",        required_argument, nullptr, 'p'},
        {"id",          required_argument, nullptr, 'i'},
        {"type",        required_argument, nullptr, 't'},
        {"no-seccomp",  no_argument,       nullptr, 'n'},
        {"help",        no_argument,       nullptr, 'H'},
        {nullptr,       0,                 nullptr, 0},
    };

    int opt;
    while ((opt = getopt_long(argc, argv, "h:p:i:t:nH", long_opts, nullptr)) != -1) {
        switch (opt) {
            case 'h': host = optarg; break;
            case 'p': port = std::stoi(optarg); break;
            case 'i': sandbox_id = optarg; break;
            case 't': target_type = optarg; break;
            case 'n': use_seccomp = false; break;
            case 'H': print_usage(argv[0]); return 0;
            default:  print_usage(argv[0]); return 1;
        }
    }

    // Generate sandbox ID if not provided
    if (sandbox_id.empty()) {
        sandbox_id = "cpp-sandbox-" + std::to_string(getpid());
    }

    std::cout << std::format("[sandbox] id={} type={} host={}:{} seccomp={}\n",
                             sandbox_id, target_type, host, port, use_seccomp);

    // Connect to Redis
    devops::RedisClient redis;
    if (!redis.connect(host, port)) {
        std::cerr << std::format("[sandbox] failed to connect to {}:{}\n", host, port);
        return 1;
    }
    std::cout << "[sandbox] connected to Redis\n";

    // Register with Redis
    auto reg = redis.sandbox_register(sandbox_id, target_type,
                                      std::format("{}:{}", host, port));
    if (!reg || reg->is_error()) {
        std::cerr << "[sandbox] failed to register\n";
        return 1;
    }
    std::cout << "[sandbox] registered\n";

    // Main loop: poll for jobs
    std::cout << "[sandbox] polling for jobs...\n";

    while (true) {
        auto job_resp = redis.job_next();
        if (!job_resp) {
            std::this_thread::sleep_for(std::chrono::milliseconds(500));
            continue;
        }

        if (job_resp->is_nil()) {
            std::this_thread::sleep_for(std::chrono::milliseconds(500));
            continue;
        }

        if (job_resp->is_error()) {
            std::cerr << std::format("[sandbox] job_next error: {}\n", job_resp->err_msg);
            std::this_thread::sleep_for(std::chrono::seconds(1));
            continue;
        }

        // Parse job: [id, name, target, command, args]
        const auto& arr = job_resp->as_arr();
        if (arr.size() < 5) {
            std::cerr << "[sandbox] malformed job response\n";
            continue;
        }

        std::string job_id = arr[0].as_str();
        std::string job_name = arr[1].as_str();
        std::string target = arr[2].as_str();
        std::string command = arr[3].as_str();
        std::string args_str = arr[4].as_str();

        std::cout << std::format("[sandbox] got job: {} ({}) target={} cmd={}\n",
                                 job_id, job_name, target, command);

        // Claim the sandbox
        redis.sandbox_claim(sandbox_id, job_id);

        // Create sandbox config
        devops::SandboxConfig cfg;
        cfg.id = job_id;
        cfg.enable_seccomp = use_seccomp;
        cfg.enable_network = (target == "network");

        // Log start
        redis.job_log(job_id, "sandbox: starting execution");

        // Run the job
        devops::Sandbox sandbox(cfg);
        auto result = sandbox.run(command, {}, [&](const std::string& line) {
            // Stream output to Redis log
            // Only send first 1000 chars per line to avoid huge payloads
            std::string truncated = line.substr(0, 1000);
            redis.job_log(job_id, truncated);
        });

        // Report result
        int exit_code = result.exit_code;
        int64_t duration_ms = result.duration_ms;

        redis.job_result(job_id, exit_code, duration_ms);
        redis.metric_record("sandbox.execution_time_ms", static_cast<double>(duration_ms));
        redis.metric_record("sandbox.exit_code", static_cast<double>(exit_code));

        std::string status = (exit_code == 0) ? "PASSED" : "FAILED";
        std::cout << std::format("[sandbox] job {} {} (exit={}, {}ms)\n",
                                 job_id, status, exit_code, duration_ms);

        // Release sandbox
        redis.sandbox_release(sandbox_id);

        // Brief pause before next job
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }

    redis.close();
    return 0;
}
