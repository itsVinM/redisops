#pragma once
#include <cstdint>
#include <string>
#include <vector>
#include <memory>
#include <filesystem>

namespace devops {

struct SnapshotConfig {
    std::string sandbox_id;
    std::string snapshot_dir = "/var/lib/sandbox/snapshots";
    bool include_memory = true;
    bool include_network = true;
    bool include_processes = true;
    bool compress = true;
};

struct SnapshotMeta {
    std::string id;
    std::string sandbox_id;
    uint64_t timestamp;
    uint64_t size_bytes;
    std::string checksum;
    std::string description;
    std::vector<std::string> included_components;
};

class SandboxSnapshot {
public:
    SandboxSnapshot(const SnapshotConfig& config);
    ~SandboxSnapshot();

    // Create a snapshot of the sandbox
    std::optional<SnapshotMeta> create(const std::string& description = "");

    // Restore from a snapshot
    bool restore(const std::string& snapshot_id);

    // List all snapshots for this sandbox
    std::vector<SnapshotMeta> list() const;

    // Delete a snapshot
    bool remove(const std::string& snapshot_id);

    // Get snapshot metadata
    std::optional<SnapshotMeta> get_meta(const std::string& snapshot_id) const;

    // Export snapshot to a file
    bool export_to(const std::string& snapshot_id, const std::string& path);

    // Import snapshot from a file
    std::optional<SnapshotMeta> import_from(const std::string& path);

private:
    bool save_process_state(const std::string& snap_path, pid_t pid);
    bool save_memory_state(const std::string& snap_path, pid_t pid);
    bool save_network_state(const std::string& snap_path);
    bool restore_process_state(const std::string& snap_path);
    bool restore_memory_state(const std::string& snap_path);
    bool restore_network_state(const std::string& snap_path);
    std::string calculate_checksum(const std::string& path);

    SnapshotConfig config_;
    std::string snapshots_dir_;
};

} // namespace devops
