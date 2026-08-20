#pragma once
#include <cstdint>
#include <string>

namespace devops {

struct CgroupConfig {
    std::string path;
    uint64_t memory_limit_bytes = 256 * 1024 * 1024;
    uint32_t cpu_quota_us = 100000;
    uint32_t cpu_period_us = 100000;
    uint32_t pids_limit = 64;
};

class Cgroup {
public:
    Cgroup(CgroupConfig config);
    ~Cgroup();

    bool create();
    bool add_pid(int pid);
    bool destroy();

private:
    CgroupConfig config_;
    bool created_ = false;
};

} // namespace devops
