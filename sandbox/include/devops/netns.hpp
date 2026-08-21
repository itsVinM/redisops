#pragma once
#include <cstdint>
#include <string>
#include <vector>
#include <functional>
#include <memory>
#include <thread>
#include <atomic>
#include <sys/types.h>

namespace devops {

struct NetnsConfig {
    std::string name;           // namespace name
    std::string bridge = "sandbox-br0";  // bridge interface
    std::string subnet = "10.100.0.0/24"; // subnet for sandboxes
    uint32_t next_ip = 2;      // next IP to assign (10.100.0.2, .3, ...)
    bool enable_loopback = true;
    bool enable_dns = true;
    std::vector<std::string> dns_servers = {"8.8.8.8", "8.8.4.4"};
};

struct NetnsState {
    std::string name;
    std::string veth_host;     // host-side veth name
    std::string veth_ns;       // namespace-side veth name
    std::string ip_addr;       // assigned IP
    std::string bridge_port;   // bridge port name
    bool active = false;
};

class NetworkNamespace {
public:
    NetworkNamespace(const NetnsConfig& config);
    ~NetworkNamespace();

    // Create and configure the namespace
    bool create();
    bool destroy();

    // Assign a process to the namespace
    bool assign_pid(pid_t pid);

    // Get namespace state
    const NetnsState& state() const { return state_; }

    // Configure iptables rules inside namespace
    bool add_iptables_rule(const std::string& rule);
    bool flush_iptables();

    // Enable/disable network for a PID
    bool enable_network_for_pid(pid_t pid);
    bool disable_network_for_pid(pid_t pid);

    // Monitoring
    struct NetStats {
        uint64_t rx_bytes;
        uint64_t tx_bytes;
        uint64_t rx_packets;
        uint64_t tx_packets;
        uint64_t rx_errors;
        uint64_t tx_errors;
    };
    std::optional<NetStats> get_stats() const;

private:
    bool create_bridge();
    bool create_veth_pair();
    bool move_to_namespace(pid_t pid);
    bool configure_namespace();
    bool setup_iptables();

    NetnsConfig config_;
    NetnsState state_;
    int netns_fd_ = -1;
};

// Manages multiple network namespaces
class NetworkManager {
public:
    NetworkManager(const NetnsConfig& base_config);
    ~NetworkManager();

    // Initialize the bridge and base network
    bool init();

    // Create a new namespace for a sandbox
    std::shared_ptr<NetworkNamespace> create_namespace(const std::string& sandbox_id);

    // Remove a namespace
    bool remove_namespace(const std::string& sandbox_id);

    // Get all namespaces
    const std::unordered_map<std::string, std::shared_ptr<NetworkNamespace>>& namespaces() const {
        return namespaces_;
    }

    // Global stats
    struct GlobalStats {
        uint64_t total_namespaces;
        uint64_t active_namespaces;
        uint64_t total_rx_bytes;
        uint64_t total_tx_bytes;
    };
    GlobalStats get_global_stats() const;

private:
    NetnsConfig base_config_;
    std::unordered_map<std::string, std::shared_ptr<NetworkNamespace>> namespaces_;
    std::atomic<uint32_t> next_ip_{2};
};

} // namespace devops
