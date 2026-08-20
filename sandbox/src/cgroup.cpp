#include <devops/cgroup.hpp>
#include <cstdio>
#include <cstring>

#ifdef __linux__
#include <sys/stat.h>
#include <unistd.h>
#endif

namespace devops {

Cgroup::Cgroup(CgroupConfig config) : config_(std::move(config)) {}

Cgroup::~Cgroup() { destroy(); }

bool Cgroup::create() {
#ifdef __linux__
    std::string base = "/sys/fs/cgroup" + config_.path;

    if (mkdir(base.c_str(), 0755) != 0 && errno != EEXIST) {
        return false;
    }

    // memory.max
    std::string p = base + "/memory.max";
    FILE* f = fopen(p.c_str(), "w");
    if (f) { fprintf(f, "%lu", config_.memory_limit_bytes); fclose(f); }

    // cpu.max
    p = base + "/cpu.max";
    f = fopen(p.c_str(), "w");
    if (f) { fprintf(f, "%u %u", config_.cpu_quota_us, config_.cpu_period_us); fclose(f); }

    // pids.max
    p = base + "/pids.max";
    f = fopen(p.c_str(), "w");
    if (f) { fprintf(f, "%u", config_.pids_limit); fclose(f); }

    created_ = true;
    return true;
#else
    return false;
#endif
}

bool Cgroup::add_pid(int pid) {
#ifdef __linux__
    if (!created_) return false;
    std::string p = "/sys/fs/cgroup" + config_.path + "/cgroup.procs";
    FILE* f = fopen(p.c_str(), "w");
    if (!f) return false;
    fprintf(f, "%d", pid);
    fclose(f);
    return true;
#else
    return false;
#endif
}

bool Cgroup::destroy() {
#ifdef __linux__
    if (!created_) return true;
    std::string base = "/sys/fs/cgroup" + config_.path;
    rmdir(base.c_str());
    created_ = false;
    return true;
#else
    return true;
#endif
}

} // namespace devops
