#ifdef __linux__

#include <devops/netns.hpp>
#include <format>
#include <iostream>
#include <fstream>
#include <sstream>
#include <cstdlib>
#include <cstring>
#include <unistd.h>
#include <sys/wait.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <fcntl.h>
#include <sched.h>
#include <net/if.h>
#include <linux/if.h>
#include <linux/if_tun.h>
#include <arpa/inet.h>

namespace devops {

// Helper: run a command and return output
static std::string run_cmd(const std::string& cmd) {
    std::string result;
    FILE* pipe = popen(cmd.c_str(), "r");
    if (!pipe) return result;
    char buf[4096];
    while (fgets(buf, sizeof(buf), pipe)) {
        result += buf;
    }
    pclose(pipe);
    return result;
}

static bool run_cmd_ok(const std::string& cmd) {
    return system(cmd.c_str()) == 0;
}

// ── NetworkNamespace ──

NetworkNamespace::NetworkNamespace(const NetnsConfig& config)
    : config_(config) {
    state_.name = config.name;
    state_.veth_host = "veth-" + config.name + "-h";
    state_.veth_ns = "veth-" + config.name + "-ns";
}

NetworkNamespace::~NetworkNamespace() {
    if (state_.active) {
        destroy();
    }
    if (netns_fd_ >= 0) {
        close(netns_fd_);
    }
}

bool NetworkNamespace::create() {
    std::string cmd = std::format("ip netns add {}", state_.name);
    if (!run_cmd_ok(cmd)) {
        std::cerr << std::format("[netns] failed to create namespace: {}\n", state_.name);
        return false;
    }

    cmd = std::format("ip link add {} type veth peer name {}",
                      state_.veth_host, state_.veth_ns);
    if (!run_cmd_ok(cmd)) {
        std::cerr << std::format("[netns] failed to create veth pair\n");
        return false;
    }

    cmd = std::format("ip link set {} netns {}", state_.veth_ns, state_.name);
    if (!run_cmd_ok(cmd)) {
        std::cerr << std::format("[netns] failed to move veth to namespace\n");
        return false;
    }

    cmd = std::format("ip netns exec {} ip addr add {}/24 dev {}",
                      state_.name, config_.subnet.substr(0, config_.subnet.find('/')),
                      state_.veth_ns);
    run_cmd_ok(cmd);

    cmd = std::format("ip netns exec {} ip link set {} up", state_.name, state_.veth_ns);
    run_cmd_ok(cmd);

    cmd = std::format("ip netns exec {} ip link set lo up", state_.name);
    run_cmd_ok(cmd);

    std::string gateway = config_.subnet.substr(0, config_.subnet.rfind('.'));
    gateway += ".1";
    cmd = std::format("ip netns exec {} ip route add default via {}", state_.name, gateway);
    run_cmd_ok(cmd);

    cmd = std::format("ip link set {} master {}", state_.veth_host, config_.bridge);
    run_cmd_ok(cmd);

    cmd = std::format("ip link set {} up", state_.veth_host);
    run_cmd_ok(cmd);

    state_.active = true;
    state_.ip_addr = config_.subnet.substr(0, config_.subnet.find('/'));
    state_.ip_addr = state_.ip_addr.substr(0, state_.ip_addr.rfind('.')) + "." +
                     std::to_string(config_.next_ip);

    std::cout << std::format("[netns] created: {} (ip: {})\n", state_.name, state_.ip_addr);
    return true;
}

bool NetworkNamespace::destroy() {
    if (!state_.active) return true;

    std::string cmd = std::format("ip link set {} nomaster", state_.veth_host);
    run_cmd_ok(cmd);

    cmd = std::format("ip link delete {}", state_.veth_host);
    run_cmd_ok(cmd);

    cmd = std::format("ip netns delete {}", state_.name);
    run_cmd_ok(cmd);

    state_.active = false;
    std::cout << std::format("[netns] destroyed: {}\n", state_.name);
    return true;
}

bool NetworkNamespace::assign_pid(pid_t pid) {
    if (!state_.active) return false;
    std::string cmd = std::format("ip netns exec {} nsenter -t {} -n true",
                                  state_.name, pid);
    return run_cmd_ok(cmd);
}

bool NetworkNamespace::add_iptables_rule(const std::string& rule) {
    if (!state_.active) return false;
    std::string cmd = std::format("ip netns exec {} iptables {}", state_.name, rule);
    return run_cmd_ok(cmd);
}

bool NetworkNamespace::flush_iptables() {
    if (!state_.active) return false;
    std::string cmd = std::format("ip netns exec {} iptables -F", state_.name);
    return run_cmd_ok(cmd);
}

bool NetworkNamespace::enable_network_for_pid(pid_t pid) {
    return assign_pid(pid);
}

bool NetworkNamespace::disable_network_for_pid(pid_t pid) {
    std::string cmd = std::format("nsenter -t {} -n ip link set lo down", pid);
    return run_cmd_ok(cmd);
}

std::optional<NetworkNamespace::NetStats> NetworkNamespace::get_stats() const {
    if (!state_.active) return std::nullopt;

    NetStats stats = {};
    std::string cmd = std::format("ip netns exec {} cat /sys/class/net/{}/statistics/",
                                  state_.name, state_.veth_ns);

    auto read_stat = [&](const std::string& name) -> uint64_t {
        std::ifstream f(cmd + name);
        uint64_t val = 0;
        f >> val;
        return val;
    };

    stats.rx_bytes = read_stat("rx_bytes");
    stats.tx_bytes = read_stat("tx_bytes");
    stats.rx_packets = read_stat("rx_packets");
    stats.tx_packets = read_stat("tx_packets");
    stats.rx_errors = read_stat("rx_errors");
    stats.tx_errors = read_stat("tx_errors");

    return stats;
}

// ── NetworkManager ──

NetworkManager::NetworkManager(const NetnsConfig& base_config)
    : base_config_(base_config) {}

NetworkManager::~NetworkManager() {
    for (auto& [id, ns] : namespaces_) {
        ns->destroy();
    }
}

bool NetworkManager::init() {
    std::string cmd = std::format("ip link show {} 2>/dev/null", base_config_.bridge);
    if (run_cmd(cmd).empty()) {
        cmd = std::format("ip link add {} type bridge", base_config_.bridge);
        if (!run_cmd_ok(cmd)) {
            std::cerr << "[netns] failed to create bridge\n";
            return false;
        }
    }

    std::string gateway = base_config_.subnet.substr(0, base_config_.subnet.rfind('.'));
    gateway += ".1/24";
    cmd = std::format("ip addr add {} dev {} 2>/dev/null", gateway, base_config_.bridge);
    run_cmd_ok(cmd);

    cmd = std::format("ip link set {} up", base_config_.bridge);
    run_cmd_ok(cmd);

    run_cmd_ok("sysctl -w net.ipv4.ip_forward=1");

    cmd = std::format("iptables -t nat -A POSTROUTING -s {} -o eth0 -j MASQUERADE",
                      base_config_.subnet);
    run_cmd_ok(cmd);

    std::cout << std::format("[netns] bridge {} ready (subnet: {})\n",
                             base_config_.bridge, base_config_.subnet);
    return true;
}

std::shared_ptr<NetworkNamespace> NetworkManager::create_namespace(const std::string& sandbox_id) {
    NetnsConfig ns_config = base_config_;
    ns_config.name = "ns-" + sandbox_id;
    ns_config.next_ip = next_ip_.fetch_add(1);

    auto ns = std::make_shared<NetworkNamespace>(ns_config);
    if (!ns->create()) {
        return nullptr;
    }

    namespaces_[sandbox_id] = ns;
    return ns;
}

bool NetworkManager::remove_namespace(const std::string& sandbox_id) {
    auto it = namespaces_.find(sandbox_id);
    if (it == namespaces_.end()) return false;

    it->second->destroy();
    namespaces_.erase(it);
    return true;
}

NetworkManager::GlobalStats NetworkManager::get_global_stats() const {
    GlobalStats stats = {};
    stats.total_namespaces = namespaces_.size();

    for (const auto& [id, ns] : namespaces_) {
        if (ns->state().active) {
            stats.active_namespaces++;
            auto ns_stats = ns->get_stats();
            if (ns_stats) {
                stats.total_rx_bytes += ns_stats->rx_bytes;
                stats.total_tx_bytes += ns_stats->tx_bytes;
            }
        }
    }

    return stats;
}

} // namespace devops

#else // Non-Linux: stubs

#include <devops/netns.hpp>
#include <iostream>

namespace devops {

NetworkNamespace::NetworkNamespace(const NetnsConfig& config) : config_(config) {
    state_.name = config.name;
}
NetworkNamespace::~NetworkNamespace() = default;
bool NetworkNamespace::create() {
    std::cerr << "[netns] network namespaces not supported on this platform\n";
    return false;
}
bool NetworkNamespace::destroy() { return true; }
bool NetworkNamespace::assign_pid(pid_t) { return false; }
bool NetworkNamespace::add_iptables_rule(const std::string&) { return false; }
bool NetworkNamespace::flush_iptables() { return false; }
bool NetworkNamespace::enable_network_for_pid(pid_t) { return false; }
bool NetworkNamespace::disable_network_for_pid(pid_t) { return false; }
std::optional<NetworkNamespace::NetStats> NetworkNamespace::get_stats() const { return std::nullopt; }

NetworkManager::NetworkManager(const NetnsConfig& config) : base_config_(config) {}
NetworkManager::~NetworkManager() = default;
bool NetworkManager::init() {
    std::cerr << "[netns] network namespaces not supported on this platform\n";
    return false;
}
std::shared_ptr<NetworkNamespace> NetworkManager::create_namespace(const std::string&) { return nullptr; }
bool NetworkManager::remove_namespace(const std::string&) { return false; }
NetworkManager::GlobalStats NetworkManager::get_global_stats() const { return {}; }

} // namespace devops

#endif
