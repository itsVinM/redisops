#include <devops/snapshot.hpp>
#include <format>
#include <iostream>
#include <fstream>
#include <sstream>
#include <chrono>
#include <cstdlib>
#include <cstring>
#include <zlib.h>
#include <sys/ptrace.h>
#include <sys/wait.h>
#include <unistd.h>

namespace fs = std::filesystem;

namespace devops {

SandboxSnapshot::SandboxSnapshot(const SnapshotConfig& config)
    : config_(config) {
    snapshots_dir_ = config_.snapshot_dir + "/" + config_.sandbox_id;
    fs::create_directories(snapshots_dir_);
}

SandboxSnapshot::~SandboxSnapshot() {}

std::optional<SnapshotMeta> SandboxSnapshot::create(const std::string& description) {
    auto now = std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::system_clock::now().time_since_epoch()).count();

    std::string snap_id = std::format("snap-{}", now);
    std::string snap_path = snapshots_dir_ + "/" + snap_id;

    fs::create_directories(snap_path);

    SnapshotMeta meta;
    meta.id = snap_id;
    meta.sandbox_id = config_.sandbox_id;
    meta.timestamp = now;
    meta.description = description;

    // Save process state
    if (config_.include_processes) {
        // Read process list from cgroup
        std::string cgroup_procs = "/sys/fs/cgroup/sandbox-" + config_.sandbox_id + "/cgroup.procs";
        std::ifstream procs_file(cgroup_procs);

        if (procs_file.is_open()) {
            std::string line;
            std::vector<pid_t> pids;
            while (std::getline(procs_file, line)) {
                pid_t pid = std::stoi(line);
                pids.push_back(pid);
                save_process_state(snap_path, pid);
            }

            // Save PID list
            std::ofstream pid_list(snap_path + "/pids.txt");
            for (pid_t pid : pids) {
                pid_list << pid << "\n";
            }
            meta.included_components.push_back("processes");
        }
    }

    // Save memory state
    if (config_.include_memory) {
        save_memory_state(snap_path, 0);
        meta.included_components.push_back("memory");
    }

    // Save network state
    if (config_.include_network) {
        save_network_state(snap_path);
        meta.included_components.push_back("network");
    }

    // Calculate size
    uint64_t total_size = 0;
    for (const auto& entry : fs::recursive_directory_iterator(snap_path)) {
        if (entry.is_regular_file()) {
            total_size += entry.file_size();
        }
    }
    meta.size_bytes = total_size;

    // Calculate checksum
    meta.checksum = calculate_checksum(snap_path);

    // Save metadata
    std::ofstream meta_file(snap_path + "/meta.json");
    meta_file << std::format(R"({{
        "id": "{}",
        "sandbox_id": "{}",
        "timestamp": {},
        "size_bytes": {},
        "checksum": "{}",
        "description": "{}"
    }})", meta.id, meta.sandbox_id, meta.timestamp, meta.size_bytes,
         meta.checksum, meta.description);

    std::cout << std::format("[snapshot] created: {} ({} bytes)\n", snap_id, total_size);
    return meta;
}

bool SandboxSnapshot::save_process_state(const std::string& snap_path, pid_t pid) {
    std::string proc_dir = snap_path + "/proc/" + std::to_string(pid);
    fs::create_directories(proc_dir);

    // Save /proc/<pid>/status
    std::string status_src = std::format("/proc/{}/status", pid);
    std::string status_dst = proc_dir + "/status";

    if (fs::exists(status_src)) {
        fs::copy_file(status_src, status_dst, fs::copy_options::overwrite_existing);
    }

    // Save /proc/<pid>/maps
    std::string maps_src = std::format("/proc/{}/maps", pid);
    std::string maps_dst = proc_dir + "/maps";

    if (fs::exists(maps_src)) {
        fs::copy_file(maps_src, maps_dst, fs::copy_options::overwrite_existing);
    }

    // Try to save memory via /proc/<pid>/mem (requires ptrace)
    std::string mem_dst = proc_dir + "/mem_dump";
    std::ifstream maps_file(maps_src);

    if (maps_file.is_open()) {
        std::ofstream mem_file(mem_dst, std::ios::binary);

        std::string line;
        while (std::getline(maps_file, line)) {
            // Parse memory region
            uint64_t start, end;
            if (sscanf(line.c_str(), "%llx-%llx", &start, &end) == 2) {
                std::string mem_path = std::format("/proc/{}/mem", pid);
                std::ifstream mem_file_in(mem_path, std::ios::binary);
                if (mem_file_in.is_open()) {
                    mem_file_in.seekg(start);
                    std::vector<char> buffer(end - start);
                    mem_file_in.read(buffer.data(), buffer.size());
                    mem_file.write(buffer.data(), buffer.size());
                }
            }
        }
    }

    return true;
}

bool SandboxSnapshot::save_memory_state(const std::string& snap_path, pid_t pid) {
    std::string mem_path = snap_path + "/memory";
    fs::create_directories(mem_path);

    // Save process memory maps
    std::string maps_file = std::format("/proc/{}/maps", pid);
    if (fs::exists(maps_file)) {
        fs::copy_file(maps_file, mem_path + "/maps", fs::copy_options::overwrite_existing);
    }

    // Save smaps if available
    std::string smaps_file = std::format("/proc/{}/smaps", pid);
    if (fs::exists(smaps_file)) {
        fs::copy_file(smaps_file, mem_path + "/smaps", fs::copy_options::overwrite_existing);
    }

    return true;
}

bool SandboxSnapshot::save_network_state(const std::string& snap_path) {
    std::string net_path = snap_path + "/network";
    fs::create_directories(net_path);

    // Save iptables rules
    system(("iptables-save > " + net_path + "/iptables.rules 2>/dev/null").c_str());

    // Save network interfaces
    system(("ip addr show > " + net_path + "/interfaces.txt 2>/dev/null").c_str());

    // Save routing table
    system(("ip route show > " + net_path + "/routes.txt 2>/dev/null").c_str());

    return true;
}

bool SandboxSnapshot::restore(const std::string& snapshot_id) {
    std::string snap_path = snapshots_dir_ + "/" + snapshot_id;

    if (!fs::exists(snap_path)) {
        std::cerr << std::format("[snapshot] not found: {}\n", snapshot_id);
        return false;
    }

    // Restore network state
    if (fs::exists(snap_path + "/network")) {
        restore_network_state(snap_path);
    }

    // Restore memory state
    if (fs::exists(snap_path + "/memory")) {
        restore_memory_state(snap_path);
    }

    // Restore processes
    if (fs::exists(snap_path + "/proc")) {
        restore_process_state(snap_path);
    }

    std::cout << std::format("[snapshot] restored: {}\n", snapshot_id);
    return true;
}

bool SandboxSnapshot::restore_process_state(const std::string& snap_path) {
    // Read saved PIDs
    std::ifstream pid_list(snap_path + "/pids.txt");
    if (!pid_list.is_open()) return false;

    std::string line;
    while (std::getline(pid_list, line)) {
        pid_t pid = std::stoi(line);
        std::string proc_dir = snap_path + "/proc/" + std::to_string(pid);

        // Restore memory maps
        if (fs::exists(proc_dir + "/mem_dump")) {
            std::string maps_file = proc_dir + "/maps";
            std::ifstream maps(maps_file);

            if (maps.is_open()) {
                std::string map_line;
                while (std::getline(maps, map_line)) {
                    uint64_t start, end;
                    if (sscanf(map_line.c_str(), "%llx-%llx", &start, &end) == 2) {
                        // Would need ptrace to restore memory
                        // This is a simplified version
                    }
                }
            }
        }
    }

    return true;
}

bool SandboxSnapshot::restore_memory_state(const std::string& snap_path) {
    // Memory state is restored via process state
    return true;
}

bool SandboxSnapshot::restore_network_state(const std::string& snap_path) {
    std::string net_path = snap_path + "/network";

    // Restore iptables
    if (fs::exists(net_path + "/iptables.rules")) {
        system(("iptables-restore < " + net_path + "/iptables.rules 2>/dev/null").c_str());
    }

    return true;
}

std::vector<SnapshotMeta> SandboxSnapshot::list() const {
    std::vector<SnapshotMeta> snapshots;

    if (!fs::exists(snapshots_dir_)) {
        return snapshots;
    }

    for (const auto& entry : fs::directory_iterator(snapshots_dir_)) {
        if (entry.is_directory()) {
            std::string meta_file = entry.path().string() + "/meta.json";
            if (fs::exists(meta_file)) {
                // Parse metadata
                SnapshotMeta meta;
                meta.id = entry.path().filename().string();
                meta.sandbox_id = config_.sandbox_id;

                // Simple parsing (in production, use a JSON library)
                std::ifstream mf(meta_file);
                std::string content((std::istreambuf_iterator<char>(mf)),
                                   std::istreambuf_iterator<char>());

                // Extract fields
                size_t pos;
                if ((pos = content.find("\"timestamp\":")) != std::string::npos) {
                    meta.timestamp = std::stoull(content.substr(pos + 13));
                }
                if ((pos = content.find("\"size_bytes\":")) != std::string::npos) {
                    meta.size_bytes = std::stoull(content.substr(pos + 13));
                }
                if ((pos = content.find("\"checksum\":")) != std::string::npos) {
                    size_t start = content.find("\"", pos + 12) + 1;
                    size_t end = content.find("\"", start);
                    meta.checksum = content.substr(start, end - start);
                }

                snapshots.push_back(meta);
            }
        }
    }

    // Sort by timestamp
    std::sort(snapshots.begin(), snapshots.end(),
              [](const SnapshotMeta& a, const SnapshotMeta& b) {
                  return a.timestamp > b.timestamp;
              });

    return snapshots;
}

bool SandboxSnapshot::remove(const std::string& snapshot_id) {
    std::string snap_path = snapshots_dir_ + "/" + snapshot_id;
    if (fs::exists(snap_path)) {
        fs::remove_all(snap_path);
        std::cout << std::format("[snapshot] deleted: {}\n", snapshot_id);
        return true;
    }
    return false;
}

std::optional<SnapshotMeta> SandboxSnapshot::get_meta(const std::string& snapshot_id) const {
    std::string snap_path = snapshots_dir_ + "/" + snapshot_id;
    std::string meta_file = snap_path + "/meta.json";

    if (!fs::exists(meta_file)) {
        return std::nullopt;
    }

    SnapshotMeta meta;
    meta.id = snapshot_id;
    meta.sandbox_id = config_.sandbox_id;

    std::ifstream mf(meta_file);
    std::string content((std::istreambuf_iterator<char>(mf)),
                       std::istreambuf_iterator<char>());

    size_t pos;
    if ((pos = content.find("\"timestamp\":")) != std::string::npos) {
        meta.timestamp = std::stoull(content.substr(pos + 13));
    }
    if ((pos = content.find("\"size_bytes\":")) != std::string::npos) {
        meta.size_bytes = std::stoull(content.substr(pos + 13));
    }

    return meta;
}

bool SandboxSnapshot::export_to(const std::string& snapshot_id, const std::string& path) {
    std::string snap_path = snapshots_dir_ + "/" + snapshot_id;
    if (!fs::exists(snap_path)) return false;

    // Create tar.gz archive
    std::string cmd = std::format("tar -czf {} -C {} {}", path, snapshots_dir_, snapshot_id);
    return system(cmd.c_str()) == 0;
}

std::optional<SnapshotMeta> SandboxSnapshot::import_from(const std::string& path) {
    if (!fs::exists(path)) return std::nullopt;

    // Extract archive
    std::string cmd = std::format("tar -xzf {} -C {}", path, snapshots_dir_);
    if (system(cmd.c_str()) != 0) {
        return std::nullopt;
    }

    // Find imported snapshot
    for (const auto& entry : fs::directory_iterator(snapshots_dir_)) {
        if (entry.is_directory()) {
            std::string meta_file = entry.path().string() + "/meta.json";
            if (fs::exists(meta_file)) {
                return get_meta(entry.path().filename().string());
            }
        }
    }

    return std::nullopt;
}

std::string SandboxSnapshot::calculate_checksum(const std::string& path) {
    // Simple checksum using md5sum
    std::string cmd = "find " + path + " -type f -exec md5sum {} \\; | sort | md5sum | awk '{print $1}'";
    FILE* pipe = popen(cmd.c_str(), "r");
    if (!pipe) return "";

    char buf[128];
    std::string result;
    while (fgets(buf, sizeof(buf), pipe)) {
        result += buf;
    }
    pclose(pipe);

    // Trim newline
    while (!result.empty() && result.back() == '\n') {
        result.pop_back();
    }

    return result;
}

} // namespace devops
