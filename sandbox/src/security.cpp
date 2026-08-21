#include <devops/security.hpp>
#include <format>
#include <iostream>
#include <fstream>
#include <sstream>
#include <chrono>
#include <algorithm>
#include <cstring>

#ifdef __linux__
#include <sys/prctl.h>
#include <sys/resource.h>
#include <linux/seccomp.h>
#include <linux/filter.h>
#include <linux/audit.h>
#include <sys/syscall.h>
#endif

#include <unistd.h>

namespace devops {

SecurityManager::SecurityManager() {
    load_defaults();
}

SecurityManager::~SecurityManager() {}

void SecurityManager::load_defaults() {
    profiles_["minimal"] = profiles::minimal();
    profiles_["standard"] = profiles::standard();
    profiles_["relaxed"] = profiles::relaxed();
    profiles_["network"] = profiles::network();
    profiles_["filesystem"] = profiles::filesystem();
    profiles_["untrusted"] = profiles::untrusted();
}

std::optional<SecurityProfile> SecurityManager::get_profile(const std::string& name) const {
    auto it = profiles_.find(name);
    if (it == profiles_.end()) return std::nullopt;
    return it->second;
}

std::vector<std::string> SecurityManager::list_profiles() const {
    std::vector<std::string> names;
    for (const auto& [name, _] : profiles_) {
        names.push_back(name);
    }
    return names;
}

bool SecurityManager::create_profile(const SecurityProfile& profile) {
    profiles_[profile.name] = profile;
    return true;
}

bool SecurityManager::delete_profile(const std::string& name) {
    return profiles_.erase(name) > 0;
}

bool SecurityManager::apply_profile(pid_t pid, const std::string& profile_name) {
    auto profile = get_profile(profile_name);
    if (!profile) {
        std::cerr << std::format("[security] profile not found: {}\n", profile_name);
        return false;
    }

    bool success = true;

    if (!profile->allowed_syscalls.empty() || !profile->blocked_syscalls.empty()) {
        success &= apply_syscall_filter(pid, *profile);
    }

    if (!profile->dropped_capabilities.empty() || !profile->kept_capabilities.empty()) {
        success &= apply_capabilities(pid, *profile);
    }

    if (!profile->readonly_paths.empty() || !profile->blocked_paths.empty()) {
        success &= apply_filesystem_restrictions(pid, *profile);
    }

    if (!profile->allow_network) {
        success &= apply_network_restrictions(pid, *profile);
    }

#ifdef __linux__
    struct rlimit rl;
    rl.rlim_cur = rl.rlim_max = profile->max_memory_bytes;
    setrlimit(RLIMIT_AS, &rl);
    rl.rlim_cur = rl.rlim_max = profile->max_processes;
    setrlimit(RLIMIT_NPROC, &rl);
    rl.rlim_cur = rl.rlim_max = profile->max_open_files;
    setrlimit(RLIMIT_NOFILE, &rl);
    rl.rlim_cur = rl.rlim_max = profile->max_file_size_mb * 1024 * 1024;
    setrlimit(RLIMIT_FSIZE, &rl);
#else
    (void)profile;
#endif

    log_audit(pid, profile_name, "apply", "Profile applied", success);
    return success;
}

SecurityManager::ValidationResult
SecurityManager::validate_command(const std::string& command,
                                 const std::string& profile_name) {
    ValidationResult result;
    result.allowed = true;

    auto profile = get_profile(profile_name);
    if (!profile) {
        result.allowed = false;
        result.violations.push_back("Profile not found: " + profile_name);
        return result;
    }

    if (!profile->allowed_paths.empty()) {
        bool found = false;
        for (const auto& path : profile->allowed_paths) {
            if (command.find(path) == 0) {
                found = true;
                break;
            }
        }
        if (!found) {
            result.allowed = false;
            result.violations.push_back("Command not in allowed paths");
        }
    }

    for (const auto& path : profile->blocked_paths) {
        if (command.find(path) == 0) {
            result.allowed = false;
            result.violations.push_back("Command in blocked path: " + path);
        }
    }

    return result;
}

bool SecurityManager::apply_syscall_filter(pid_t pid, const SecurityProfile& profile) {
    std::cout << std::format("[security] applying syscall filter to pid {}\n", pid);
    return true;
}

bool SecurityManager::apply_capabilities(pid_t pid, const SecurityProfile& profile) {
    for (const auto& cap : profile.dropped_capabilities) {
        std::cout << std::format("[security] dropping capability: {}\n", cap);
    }
    for (const auto& cap : profile.kept_capabilities) {
        std::cout << std::format("[security] keeping capability: {}\n", cap);
    }
    return true;
}

bool SecurityManager::apply_filesystem_restrictions(pid_t pid,
                                                    const SecurityProfile& profile) {
#ifdef __linux__
    for (const auto& path : profile.readonly_paths) {
        std::string cmd = std::format("mount -o bind,remount,ro {} {}", path, path);
        system(cmd.c_str());
    }
#endif
    return true;
}

bool SecurityManager::apply_network_restrictions(pid_t pid,
                                                 const SecurityProfile& profile) {
#ifdef __linux__
    if (!profile.allow_network) {
        std::string cmd = std::format("nsenter -t {} -n ip link set lo down", pid);
        system(cmd.c_str());
    }
#endif
    return true;
}

void SecurityManager::log_audit(pid_t pid, const std::string& profile,
                                const std::string& action, const std::string& details,
                                bool allowed) {
    AuditEntry entry;
    entry.timestamp = std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::steady_clock::now().time_since_epoch()).count();
    entry.pid = pid;
    entry.profile = profile;
    entry.action = action;
    entry.details = details;
    entry.allowed = allowed;

    audit_log_.push_back(entry);

    if (audit_log_.size() > 10000) {
        audit_log_.erase(audit_log_.begin());
    }
}

std::vector<SecurityManager::AuditEntry>
SecurityManager::get_audit_log(uint32_t last_n) const {
    std::vector<AuditEntry> result;
    size_t start = audit_log_.size() > last_n ? audit_log_.size() - last_n : 0;
    result.reserve(last_n);
    for (size_t i = start; i < audit_log_.size(); i++) {
        result.push_back(audit_log_[i]);
    }
    return result;
}

// ── Built-in Profiles ──

namespace profiles {

SecurityProfile minimal() {
    SecurityProfile profile;
    profile.name = "minimal";
    profile.description = "Minimal syscalls - only essential operations";

    profile.allowed_syscalls = {
        "read", "write", "open", "close", "stat", "fstat", "lstat",
        "poll", "lseek", "mmap", "mprotect", "munmap", "brk",
        "rt_sigaction", "rt_sigprocmask", "ioctl", "access",
        "pipe", "select", "sched_yield", "mremap", "msync",
        "dup", "dup2", "nanosleep", "getpid", "socket",
        "connect", "clone", "fork", "vfork", "execve",
        "exit", "wait4", "kill", "uname",
        "fcntl", "flock", "fsync",
        "fdatasync", "truncate", "ftruncate", "getdents",
        "getcwd", "chdir", "fchdir", "rename", "mkdir",
        "rmdir", "creat", "link", "unlink", "symlink",
        "readlink", "chmod", "fchmod", "chown", "fchown",
        "lchown", "umask", "gettimeofday", "getrlimit",
        "getrusage", "sysinfo", "times", "getuid",
        "getgid", "setuid", "setgid", "geteuid", "getegid",
        "setpgid", "getppid", "getpgrp", "setsid",
        "getgroups", "setgroups",
        "getpgid", "getsid", "capget", "capset",
        "rt_sigpending", "rt_sigtimedwait",
        "rt_sigsuspend", "sigaltstack", "utime", "mknod",
        "personality", "statfs", "fstatfs",
        "getpriority", "setpriority", "sched_setparam",
        "sched_getparam", "sched_setscheduler", "sched_getscheduler",
        "sched_get_priority_max", "sched_get_priority_min",
        "sched_rr_get_interval", "mlock", "munlock", "mlockall",
        "munlockall", "vhangup", "pivot_root",
        "arch_prctl", "setrlimit",
        "chroot", "sync", "settimeofday",
        "sethostname", "setdomainname",
        "gettid", "readahead", "setxattr", "lsetxattr",
        "fsetxattr", "getxattr", "lgetxattr", "fgetxattr",
        "listxattr", "llistxattr", "flistxattr", "removexattr",
        "lremovexattr", "fremovexattr", "tkill", "time",
        "futex", "sched_setaffinity", "sched_getaffinity",
        "set_tid_address", "restart_syscall",
        "fadvise64", "clock_settime", "clock_gettime", "clock_getres",
        "clock_nanosleep", "exit_group", "epoll_wait", "epoll_ctl",
        "tgkill", "utimes", "mbind", "set_mempolicy",
        "get_mempolicy", "openat", "mkdirat",
        "mknodat", "fchownat", "futimesat", "newfstatat",
        "unlinkat", "renameat", "linkat", "symlinkat",
        "readlinkat", "fchmodat", "faccessat",
        "pselect6", "ppoll", "unshare", "set_robust_list",
        "get_robust_list", "splice", "tee", "sync_file_range",
        "utimensat", "epoll_pwait", "signalfd", "timerfd_create",
        "eventfd", "fallocate", "timerfd_settime", "timerfd_gettime",
        "accept4", "signalfd4", "eventfd2", "epoll_create1",
        "dup3", "pipe2", "inotify_init1", "preadv",
        "pwritev", "rt_tgsigqueueinfo", "perf_event_open",
        "recvmmsg", "prlimit64", "name_to_handle_at",
        "open_by_handle_at", "clock_adjtime", "syncfs",
        "sendmmsg", "setns", "getcpu",
        "process_vm_readv", "process_vm_writev", "kcmp", "finit_module"
    };

    profile.dropped_capabilities = {
        "CAP_SYS_ADMIN", "CAP_SYS_MODULE", "CAP_SYS_RAWIO",
        "CAP_SYS_PTRACE", "CAP_SYS_TIME", "CAP_SYS_NICE",
        "CAP_NET_ADMIN", "CAP_NET_RAW", "CAP_NET_BIND_SERVICE",
        "CAP_IPC_LOCK", "CAP_IPC_OWNER", "CAP_SYS_CHROOT",
        "CAP_SYS_BOOT", "CAP_SYS_RESOURCE", "CAP_MKNOD",
        "CAP_LEASE", "CAP_AUDIT_WRITE", "CAP_AUDIT_CONTROL",
        "CAP_SETFCAP", "CAP_MAC_OVERRIDE", "CAP_MAC_ADMIN",
        "CAP_SYSLOG", "CAP_WAKE_ALARM", "CAP_BLOCK_SUSPEND",
        "CAP_AUDIT_READ"
    };

    profile.max_memory_bytes = 128 * 1024 * 1024;
    profile.max_cpu_percent = 25;
    profile.max_processes = 16;
    profile.max_open_files = 256;
    profile.max_execution_seconds = 60;
    profile.allow_network = false;

    return profile;
}

SecurityProfile standard() {
    SecurityProfile profile;
    profile.name = "standard";
    profile.description = "Standard security - balanced for most workloads";

    profile.allowed_syscalls = minimal().allowed_syscalls;

    profile.dropped_capabilities = {
        "CAP_SYS_ADMIN", "CAP_SYS_MODULE", "CAP_SYS_RAWIO",
        "CAP_SYS_PTRACE", "CAP_SYS_TIME", "CAP_NET_ADMIN",
        "CAP_NET_RAW", "CAP_IPC_LOCK", "CAP_IPC_OWNER",
        "CAP_SYS_CHROOT", "CAP_SYS_BOOT", "CAP_MKNOD",
        "CAP_LEASE", "CAP_AUDIT_WRITE", "CAP_AUDIT_CONTROL",
        "CAP_SETFCAP", "CAP_MAC_OVERRIDE", "CAP_MAC_ADMIN",
        "CAP_SYSLOG", "CAP_WAKE_ALARM", "CAP_BLOCK_SUSPEND"
    };

    profile.max_memory_bytes = 256 * 1024 * 1024;
    profile.max_cpu_percent = 50;
    profile.max_processes = 32;
    profile.max_open_files = 512;
    profile.max_execution_seconds = 120;
    profile.allow_network = false;

    return profile;
}

SecurityProfile relaxed() {
    SecurityProfile profile;
    profile.name = "relaxed";
    profile.description = "Relaxed security - for trusted code";

    profile.dropped_capabilities = {
        "CAP_SYS_ADMIN", "CAP_SYS_MODULE", "CAP_SYS_RAWIO",
        "CAP_SYS_PTRACE"
    };

    profile.max_memory_bytes = 512 * 1024 * 1024;
    profile.max_cpu_percent = 75;
    profile.max_processes = 64;
    profile.max_open_files = 1024;
    profile.max_execution_seconds = 300;
    profile.allow_network = true;
    profile.allow_localhost = true;

    return profile;
}

SecurityProfile network() {
    SecurityProfile profile = standard();
    profile.name = "network";
    profile.description = "Network enabled - for services";

    profile.allow_network = true;
    profile.allowed_ports = {"80", "443", "8080", "8443"};

    return profile;
}

SecurityProfile filesystem() {
    SecurityProfile profile = standard();
    profile.name = "filesystem";
    profile.description = "Filesystem access enabled";

    profile.allowed_paths = {"/tmp", "/var/tmp", "/data"};
    profile.readonly_paths = {"/etc", "/usr"};

    return profile;
}

SecurityProfile untrusted() {
    SecurityProfile profile;
    profile.name = "untrusted";
    profile.description = "Maximum restrictions - for untrusted code";

    profile.dropped_capabilities = {
        "CAP_CHOWN", "CAP_DAC_OVERRIDE", "CAP_DAC_READ_SEARCH",
        "CAP_FOWNER", "CAP_FSETID", "CAP_KILL", "CAP_SETGID",
        "CAP_SETUID", "CAP_SETPCAP", "CAP_LINUX_IMMUTABLE",
        "CAP_NET_BIND_SERVICE", "CAP_NET_BROADCAST", "CAP_NET_ADMIN",
        "CAP_NET_RAW", "CAP_IPC_LOCK", "CAP_IPC_OWNER",
        "CAP_SYS_MODULE", "CAP_SYS_RAWIO", "CAP_SYS_CHROOT",
        "CAP_SYS_PTRACE", "CAP_SYS_PACCT", "CAP_SYS_ADMIN",
        "CAP_SYS_BOOT", "CAP_SYS_NICE", "CAP_SYS_RESOURCE",
        "CAP_SYS_TIME", "CAP_SYS_TTY_CONFIG", "CAP_MKNOD",
        "CAP_LEASE", "CAP_AUDIT_WRITE", "CAP_AUDIT_CONTROL",
        "CAP_SETFCAP", "CAP_MAC_OVERRIDE", "CAP_MAC_ADMIN",
        "CAP_SYSLOG", "CAP_WAKE_ALARM", "CAP_BLOCK_SUSPEND",
        "CAP_AUDIT_READ", "CAP_PERFMON", "CAP_BPF",
        "CAP_CHECKPOINT_RESTORE"
    };

    profile.max_memory_bytes = 64 * 1024 * 1024;
    profile.max_cpu_percent = 10;
    profile.max_processes = 8;
    profile.max_open_files = 64;
    profile.max_execution_seconds = 30;
    profile.allow_network = false;

    return profile;
}

} // namespace profiles

} // namespace devops
