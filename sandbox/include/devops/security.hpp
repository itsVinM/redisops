#pragma once
#include <cstdint>
#include <string>
#include <vector>
#include <unordered_set>

namespace devops {

struct SecurityProfile {
    std::string name;
    std::string description;

    // Syscall whitelist (empty = use default)
    std::unordered_set<std::string> allowed_syscalls;

    // Syscall blacklist
    std::unordered_set<std::string> blocked_syscalls;

    // Capabilities to drop
    std::unordered_set<std::string> dropped_capabilities;

    // Capabilities to keep
    std::unordered_set<std::string> kept_capabilities;

    // Filesystem restrictions
    std::vector<std::string> readonly_paths;
    std::vector<std::string> blocked_paths;
    std::vector<std::string> allowed_paths;

    // Network restrictions
    bool allow_network = false;
    bool allow_localhost = false;
    std::vector<std::string> allowed_hosts;
    std::vector<std::string> allowed_ports;

    // Resource limits
    uint64_t max_memory_bytes = 256 * 1024 * 1024;
    uint32_t max_cpu_percent = 50;
    uint32_t max_processes = 64;
    uint32_t max_open_files = 1024;
    uint32_t max_file_size_mb = 100;

    // Time limits
    uint32_t max_execution_seconds = 300;

    // Output limits
    uint32_t max_stdout_bytes = 1024 * 1024;
    uint32_t max_stderr_bytes = 1024 * 1024;
};

class SecurityManager {
public:
    SecurityManager();
    ~SecurityManager();

    // Load built-in profiles
    void load_defaults();

    // Get a profile by name
    std::optional<SecurityProfile> get_profile(const std::string& name) const;

    // List all profiles
    std::vector<std::string> list_profiles() const;

    // Create custom profile
    bool create_profile(const SecurityProfile& profile);

    // Delete profile
    bool delete_profile(const std::string& name);

    // Apply profile to a process
    bool apply_profile(pid_t pid, const std::string& profile_name);

    // Validate a command against a profile
    struct ValidationResult {
        bool allowed;
        std::vector<std::string> violations;
    };
    ValidationResult validate_command(const std::string& command,
                                     const std::string& profile_name);

    // Audit log
    struct AuditEntry {
        uint64_t timestamp;
        pid_t pid;
        std::string profile;
        std::string action;
        std::string details;
        bool allowed;
    };
    std::vector<AuditEntry> get_audit_log(uint32_t last_n = 100) const;

private:
    bool apply_syscall_filter(pid_t pid, const SecurityProfile& profile);
    bool apply_capabilities(pid_t pid, const SecurityProfile& profile);
    bool apply_filesystem_restrictions(pid_t pid, const SecurityProfile& profile);
    bool apply_network_restrictions(pid_t pid, const SecurityProfile& profile);

    void log_audit(pid_t pid, const std::string& profile, const std::string& action,
                  const std::string& details, bool allowed);

    std::unordered_map<std::string, SecurityProfile> profiles_;
    std::vector<AuditEntry> audit_log_;
};

// Built-in security profiles
namespace profiles {

// Minimal: only essential syscalls
SecurityProfile minimal();

// Standard: balanced security and functionality
SecurityProfile standard();

// Relaxed: more permissive for development
SecurityProfile relaxed();

// Network: allows network access
SecurityProfile network();

// IO: allows file system access
SecurityProfile filesystem();

// Untrusted: maximum restrictions
SecurityProfile untrusted();

} // namespace profiles

} // namespace devops
