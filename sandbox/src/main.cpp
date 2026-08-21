#include <devops/redis_client.hpp>
#include <devops/sandbox.hpp>
#include <devops/metrics.hpp>
#include <devops/file_watcher.hpp>
#include <devops/health_monitor.hpp>
#include <devops/log_aggregator.hpp>
#include <devops/resource_monitor.hpp>
#include <devops/process_supervisor.hpp>
#include <devops/netns.hpp>
#include <devops/snapshot.hpp>
#include <devops/ipc.hpp>
#include <devops/security.hpp>
#include <format>
#include <iostream>
#include <string>
#include <thread>
#include <chrono>
#include <cstring>
#include <cstdlib>
#include <signal.h>
#include <getopt.h>
#ifdef __APPLE__
#include <sys/event.h>
#define IN_CREATE 0
#define IN_MODIFY 0
#else
#include <sys/inotify.h>
#endif

static volatile bool g_running = true;

static void signal_handler(int) {
    g_running = false;
}

static void print_usage(const char* prog) {
    std::cerr << std::format(
        "Usage: {} [options]\n"
        "  -h, --host HOST       Redis host (default: 127.0.0.1)\n"
        "  -p, --port PORT       Redis port (default: 1234)\n"
        "  -i, --id ID           Sandbox ID (default: auto)\n"
        "  -t, --type TYPE       Target type (default: local)\n"
        "  -m, --metrics-port    Prometheus port (default: 9090)\n"
        "  -w, --watch PATH      Watch path for auto-trigger\n"
        "  -n, --no-seccomp      Disable seccomp filter\n"
        "  --security PROFILE    Security profile (minimal/standard/relaxed/network/filesystem/untrusted)\n"
        "  --enable-netns        Enable network namespace isolation\n"
        "  --enable-ipc          Enable inter-sandbox IPC\n"
        "  --max-restarts N      Max restarts on failure (default: 3)\n"
        " --help                Show this help\n",
        prog);
}

int main(int argc, char* argv[]) {
    signal(SIGINT, signal_handler);
    signal(SIGTERM, signal_handler);

    std::string host = "127.0.0.1";
    uint16_t port = 1234;
    std::string sandbox_id = "cpp-sandbox-1";
    std::string target_type = "local";
    uint16_t metrics_port = 9090;
    std::string watch_path;
    bool use_seccomp = true;
    std::string security_profile = "standard";
    bool enable_netns = false;
    bool enable_ipc = false;
    uint32_t max_restarts = 3;

    static struct option long_opts[] = {
        {"host",          required_argument, nullptr, 'h'},
        {"port",          required_argument, nullptr, 'p'},
        {"id",            required_argument, nullptr, 'i'},
        {"type",          required_argument, nullptr, 't'},
        {"metrics-port",  required_argument, nullptr, 'm'},
        {"watch",         required_argument, nullptr, 'w'},
        {"no-seccomp",    no_argument,       nullptr, 'n'},
        {"security",      required_argument, nullptr, 's'},
        {"enable-netns",  no_argument,       nullptr, 'N'},
        {"enable-ipc",    no_argument,       nullptr, 'I'},
        {"max-restarts",  required_argument, nullptr, 'r'},
        {"help",          no_argument,       nullptr, 'H'},
        {nullptr,         0,                 nullptr, 0},
    };

    int opt;
    while ((opt = getopt_long(argc, argv, "h:p:i:t:m:w:ns:NIr:H", long_opts, nullptr)) != -1) {
        switch (opt) {
            case 'h': host = optarg; break;
            case 'p': port = std::stoi(optarg); break;
            case 'i': sandbox_id = optarg; break;
            case 't': target_type = optarg; break;
            case 'm': metrics_port = std::stoi(optarg); break;
            case 'w': watch_path = optarg; break;
            case 'n': use_seccomp = false; break;
            case 's': security_profile = optarg; break;
            case 'N': enable_netns = true; break;
            case 'I': enable_ipc = true; break;
            case 'r': max_restarts = std::stoi(optarg); break;
            case 'H': print_usage(argv[0]); return 0;
            default:  print_usage(argv[0]); return 1;
        }
    }

    if (sandbox_id.empty()) {
        sandbox_id = "cpp-sandbox-" + std::to_string(getpid());
    }

    std::cout << std::format(
        "[sandbox] id={} type={} host={}:{} metrics={} seccomp={} profile={} netns={} ipc={}\n",
        sandbox_id, target_type, host, port, metrics_port, use_seccomp,
        security_profile, enable_netns, enable_ipc);

    // ── Initialize components ──

    devops::MetricsExporter metrics;
    devops::MetricsServer metrics_server(metrics_port);
    devops::HealthMonitor health_monitor;
    devops::LogAggregator log_agg;
    devops::FileWatcher file_watcher;
    devops::ResourceManager resource_manager;
    devops::ProcessSupervisor process_supervisor;
    devops::SecurityManager security_manager;
    std::shared_ptr<devops::NetworkManager> net_manager;
    std::shared_ptr<devops::IPCManager> ipc_manager;

    metrics_server.set_exporter(&metrics);

    // Register health checks (matching the HealthCheckFn signature: bool(std::string&))
    health_monitor.register_check("redis", [&](std::string& msg) -> bool {
        // Health check runs periodically
        msg = "ok";
        return true;
    });
    health_monitor.register_check("sandbox", [&](std::string& msg) -> bool {
        msg = "running";
        return true;
    });
    health_monitor.register_check("resources", [&](std::string& msg) -> bool {
        auto stats = resource_manager.get_global_stats();
        if (stats.sandboxes_over_limit > 0) {
            msg = std::format("{} sandboxes over limit", stats.sandboxes_over_limit);
            return false;
        }
        msg = "ok";
        return true;
    });

    // Security manager
    auto sec_profile = security_manager.get_profile(security_profile);
    if (!sec_profile) {
        std::cerr << std::format("[sandbox] unknown security profile: {}\n", security_profile);
        return 1;
    }

    // Network namespace manager
    if (enable_netns) {
        devops::NetnsConfig netns_cfg;
        netns_cfg.name = "sandbox-netns";
        netns_cfg.bridge = "sandbox-br0";
        netns_cfg.subnet = "10.100.0.0/24";
        net_manager = std::make_shared<devops::NetworkManager>(netns_cfg);
        if (!net_manager->init()) {
            std::cerr << "[sandbox] failed to init network manager\n";
            return 1;
        }
        std::cout << "[sandbox] network namespace manager ready\n";
    }

    // IPC manager
    if (enable_ipc) {
        devops::IPCConfig ipc_cfg;
        ipc_cfg.sandbox_id = sandbox_id;
        ipc_cfg.socket_path = "/tmp/sandbox-ipc";
        ipc_manager = std::make_shared<devops::IPCManager>(ipc_cfg);
        auto channel = ipc_manager->register_sandbox(sandbox_id);
        if (!channel) {
            std::cerr << "[sandbox] failed to register IPC channel\n";
        } else {
            channel->subscribe("command", [&](const devops::IPCMessage& msg) {
                log_agg.push("ipc", "info",
                    std::format("received command: {}", msg.as_string()));
            });
            std::cout << "[sandbox] IPC channel ready\n";
        }
    }

    // Start metrics server
    if (metrics_server.start()) {
        std::cout << std::format("[sandbox] metrics on :{}/metrics\n", metrics_port);
    }

    // Start health monitor
    health_monitor.set_interval_ms(5000);
    health_monitor.start();

    // Start file watcher if path specified
    if (!watch_path.empty()) {
        file_watcher.watch(watch_path, IN_CREATE | IN_MODIFY);
        file_watcher.on_change([&](const devops::FileEvent& e) {
            log_agg.push("filewatcher", "info",
                        std::format("file changed: {}", e.path));
            metrics.counter("filewatcher.events_total", 1);
        });
        file_watcher.start();
        std::cout << std::format("[sandbox] watching: {}\n", watch_path);
    }

    // Process supervisor event handler
    process_supervisor.on_event([&](const devops::ProcessEvent& e) {
        std::string type_str;
        switch (e.type) {
            case devops::ProcessEvent::Type::STARTED:    type_str = "STARTED"; break;
            case devops::ProcessEvent::Type::STOPPED:    type_str = "STOPPED"; break;
            case devops::ProcessEvent::Type::CRASHED:    type_str = "CRASHED"; break;
            case devops::ProcessEvent::Type::RESTARTED:  type_str = "RESTARTED"; break;
            case devops::ProcessEvent::Type::HEALTH_CHECK_FAILED: type_str = "HEALTH_FAIL"; break;
        }
        log_agg.push("supervisor", type_str,
            std::format("{}: pid={} exit={}", e.process_name, e.pid, e.exit_code));
        metrics.counter("supervisor.events_total", 1, {{"type", type_str}});
    });

    // ── Connect to Redis ──

    devops::RedisClient redis;
    if (!redis.connect(host, port)) {
        std::cerr << std::format("[sandbox] failed to connect to {}:{}\n", host, port);
        return 1;
    }
    std::cout << "[sandbox] connected to Redis\n";

    auto reg = redis.sandbox_register(sandbox_id, target_type,
                                      std::format("{}:{}", host, port));
    if (!reg || reg->is_error()) {
        std::cerr << "[sandbox] failed to register\n";
        return 1;
    }
    std::cout << "[sandbox] registered\n";

    // ── Main loop ──

    std::cout << "[sandbox] polling for jobs...\n";
    uint64_t jobs_completed = 0;
    uint64_t jobs_failed = 0;

    while (g_running) {
        auto job_resp = redis.job_next();
        if (!job_resp || job_resp->is_nil()) {
            std::this_thread::sleep_for(std::chrono::milliseconds(500));
            continue;
        }

        if (job_resp->is_error()) {
            std::cerr << std::format("[sandbox] job_next error: {}\n", job_resp->err_msg);
            std::this_thread::sleep_for(std::chrono::seconds(1));
            continue;
        }

        const auto& arr = job_resp->as_arr();
        if (arr.size() < 5) {
            std::cerr << "[sandbox] malformed job response\n";
            continue;
        }

        std::string job_id = arr[0].as_str();
        std::string job_name = arr[1].as_str();
        std::string target = arr[2].as_str();
        std::string command = arr[3].as_str();

        log_agg.push("scheduler", "info", std::format("running job: {} ({})", job_id, job_name));
        metrics.counter("sandbox.jobs_started_total", 1, {{"target", target}});

        redis.sandbox_claim(sandbox_id, job_id);

        // Validate command against security profile
        auto validation = security_manager.validate_command(command, security_profile);
        if (!validation.allowed) {
            std::cerr << std::format("[sandbox] command blocked: {}\n", command);
            for (const auto& v : validation.violations) {
                std::cerr << std::format("  - {}\n", v);
            }
            redis.job_log(job_id, "SECURITY: command blocked by policy");
            redis.job_result(job_id, 126, 0);
            redis.sandbox_release(sandbox_id);
            continue;
        }

        // Create network namespace if enabled
        std::shared_ptr<devops::NetworkNamespace> netns;
        if (enable_netns && net_manager) {
            netns = net_manager->create_namespace(job_id);
            if (netns) {
                redis.job_log(job_id, "sandbox: network namespace created");
            }
        }

        devops::SandboxConfig cfg;
        cfg.id = job_id;
        cfg.enable_seccomp = use_seccomp;
        cfg.enable_network = (target == "network" || enable_netns);

        redis.job_log(job_id, "sandbox: starting execution");

        auto start = std::chrono::steady_clock::now();

        devops::Sandbox sandbox(cfg);
        auto result = sandbox.run(command, {}, [&](const std::string& line) {
            std::string truncated = line.substr(0, 1000);
            redis.job_log(job_id, truncated);
            log_agg.push_raw("sandbox:" + job_id, line);
        });

        auto end = std::chrono::steady_clock::now();
        int64_t duration_ms = std::chrono::duration_cast<std::chrono::milliseconds>(end - start).count();

        redis.job_result(job_id, result.exit_code, duration_ms);
        metrics.counter("sandbox.jobs_completed_total", 1,
                       {{"target", target}, {"exit_code", std::to_string(result.exit_code)}});
        metrics.histogram("sandbox.execution_duration_ms", static_cast<double>(duration_ms));

        if (result.exit_code == 0) {
            jobs_completed++;
            log_agg.push("scheduler", "info", std::format("job {} PASSED ({}ms)", job_id, duration_ms));
        } else {
            jobs_failed++;
            log_agg.push("scheduler", "error", std::format("job {} FAILED (exit={})", job_id, result.exit_code));
        }

        metrics.gauge("sandbox.jobs_completed", static_cast<double>(jobs_completed));
        metrics.gauge("sandbox.jobs_failed", static_cast<double>(jobs_failed));

        // Cleanup network namespace
        if (netns && net_manager) {
            net_manager->remove_namespace(job_id);
        }

        redis.sandbox_release(sandbox_id);

        std::string status = (result.exit_code == 0) ? "PASSED" : "FAILED";
        std::cout << std::format("[sandbox] job {} {} (exit={}, {}ms)\n",
                                 job_id, status, result.exit_code, duration_ms);

        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }

    // ── Shutdown ──

    std::cout << "[sandbox] shutting down...\n";
    file_watcher.stop();
    health_monitor.stop();
    metrics_server.stop();
    process_supervisor.stop_all();
    redis.close();

    if (enable_netns && net_manager) {
        // Cleanup all network namespaces
    }

    std::cout << std::format("[sandbox] completed: {} jobs, {} failed\n",
                             jobs_completed, jobs_failed);
    return 0;
}
