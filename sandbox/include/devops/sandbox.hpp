#pragma once
#include <cstdint>
#include <string>
#include <functional>

namespace devops {

struct SandboxConfig {
    std::string id;
    std::string rootfs = "/";
    uint64_t memory_limit_bytes = 256 * 1024 * 1024;  // 256 MB
    uint32_t cpu_quota_us = 100000;                     // 100ms per 100ms period
    uint32_t cpu_period_us = 100000;
    uint32_t pids_limit = 64;
    bool enable_network = false;
    bool enable_seccomp = true;
};

struct ExecResult {
    int exit_code;
    int64_t duration_ms;
    std::string stdout;
    std::string stderr;
};

class Sandbox {
public:
    Sandbox(SandboxConfig config);
    ~Sandbox();

    // Run a command in the sandbox, call output_fn with each line of output
    ExecResult run(const std::string& command, const std::vector<std::string>& args,
                   std::function<void(const std::string&)> output_fn = nullptr);

private:
    bool setup_namespaces();
    bool setup_cgroup();
    bool setup_mounts();
    bool install_seccomp();
    void cleanup();

    SandboxConfig config_;
    std::string cgroup_path_;
    pid_t child_pid_ = -1;
    int pipe_fd_[2] = {-1, -1};
};

} // namespace devops
