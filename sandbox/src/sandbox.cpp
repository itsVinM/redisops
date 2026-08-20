#include <devops/sandbox.hpp>
#include <format>
#include <iostream>
#include <cstring>
#include <chrono>

#ifdef __linux__
#include <sched.h>
#include <sys/wait.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>
#include <fcntl.h>
#include <signal.h>
#include <seccomp.h>
#include <linux/seccomp.h>
#endif

namespace devops {

Sandbox::Sandbox(SandboxConfig config) : config_(std::move(config)) {}

Sandbox::~Sandbox() { cleanup(); }

void Sandbox::cleanup() {
#ifdef __linux__
    if (cgroup_path_.size() > 1) {
        std::string cg_path = "/sys/fs/cgroup" + cgroup_path_;
        rmdir(cg_path.c_str());
    }
    if (pipe_fd_[0] >= 0) close(pipe_fd_[0]);
    if (pipe_fd_[1] >= 0) close(pipe_fd_[1]);
    pipe_fd_[0] = pipe_fd_[1] = -1;
#endif
}

#ifdef __linux__

// ── Stack allocator for clone() ──

static void* alloc_stack(size_t size) {
    void* stack = malloc(size);
    if (!stack) return nullptr;
    return static_cast<char*>(stack) + size;
}

// ── Data passed into the child ──

struct ChildArgs {
    SandboxConfig config;
    std::string cgroup_path;
    int pipe_fd;
};

// ── Child entry point (called in new PID namespace) ──

static int child_entry(void* arg) {
    auto* args = static_cast<ChildArgs*>(arg);

    // Setup cgroup
    std::string cg_base = "/sys/fs/cgroup" + args->cgroup_path;
    mkdir(cg_base.c_str(), 0755);

    auto write_file = [](const std::string& path, const char* fmt, auto... vals) {
        FILE* f = fopen(path.c_str(), "w");
        if (f) { fprintf(f, fmt, vals...); fclose(f); }
    };

    write_file(cg_base + "/memory.max", "%lu", args->config.memory_limit_bytes);
    write_file(cg_base + "/cpu.max", "%u %u", args->config.cpu_quota_us, args->config.cpu_period_us);
    write_file(cg_base + "/pids.max", "%u", args->config.pids_limit);
    write_file(cg_base + "/cgroup.procs", "%d", getpid());

    // Install seccomp filter
    if (args->config.enable_seccomp) {
        scmp_filter_ctx ctx = seccomp_init(SCMP_ACT_KILL);
        if (ctx) {
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(read), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(write), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(exit), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(exit_group), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(brk), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(mmap), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(munmap), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(close), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(fstat), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(clock_gettime), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(rt_sigaction), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(rt_sigprocmask), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(clone), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(execve), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(wait4), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(dup2), 0);
            seccomp_rule_add(ctx, SCMP_ACT_ALLOW, SCMP_SYS(fcntl), 0);
            seccomp_load(ctx);
            seccomp_release(ctx);
        }
    }

    // Redirect stdout/stderr to pipe
    dup2(args->pipe_fd, STDOUT_FILENO);
    dup2(args->pipe_fd, STDERR_FILENO);
    close(args->pipe_fd);

    // Exec
    execl("/bin/sh", "sh", "-c", args->config.rootfs.c_str(), nullptr);
    _exit(127);
}

bool Sandbox::setup_cgroup() {
    cgroup_path_ = "/devops-" + config_.id;
    return true;
}

ExecResult Sandbox::run(const std::string& command, const std::vector<std::string>&,
                        std::function<void(const std::string&)> output_fn) {
    ExecResult result{};

    auto start = std::chrono::steady_clock::now();

    setup_cgroup();

    if (pipe(pipe_fd_) != 0) {
        result.exit_code = -1;
        result.stderr = "pipe() failed";
        return result;
    }

    // Build child args — must outlive the clone call
    ChildArgs child_args;
    child_args.config = config_;
    child_args.config.rootfs = command;  // command becomes the shell -c arg
    child_args.cgroup_path = cgroup_path_;
    child_args.pipe_fd = pipe_fd_[1];

    // Clone into new PID namespace
    unsigned long flags = CLONE_NEWPID | SIGCHLD;
    if (!config_.enable_network) {
        flags |= CLONE_NEWNET;
    }

    void* stack = alloc_stack(4096);
    child_pid_ = clone(child_entry, stack, flags, &child_args);

    if (child_pid_ < 0) {
        result.exit_code = -1;
        result.stderr = std::format("clone() failed: {}", strerror(errno));
        free(static_cast<char*>(stack) - 4096);
        cleanup();
        return result;
    }

    // Free the stack (child has its own copy)
    free(static_cast<char*>(stack) - 4096);

    // Close write end in parent
    close(pipe_fd_[1]);
    pipe_fd_[1] = -1;

    // Read output from child
    char buf[4096];
    ssize_t n;
    while ((n = read(pipe_fd_[0], buf, sizeof(buf) - 1)) > 0) {
        buf[n] = '\0';
        std::string line(buf, n);
        if (output_fn) output_fn(line);
        result.stdout += line;
    }

    // Wait for child
    int status;
    waitpid(child_pid_, &status, 0);

    auto end = std::chrono::steady_clock::now();
    result.duration_ms = std::chrono::duration_cast<std::chrono::milliseconds>(end - start).count();

    if (WIFEXITED(status)) {
        result.exit_code = WEXITSTATUS(status);
    } else if (WIFSIGNALED(status)) {
        result.exit_code = 128 + WTERMSIG(status);
    } else {
        result.exit_code = -1;
    }

    cleanup();
    return result;
}

#else

// Non-Linux stub
ExecResult Sandbox::run(const std::string&, const std::vector<std::string>&,
                        std::function<void(const std::string&)> output_fn) {
    ExecResult result{};
    result.exit_code = -1;
    result.stderr = "sandbox only supported on Linux (use Docker or VM for macOS)";
    return result;
}

#endif

} // namespace devops
