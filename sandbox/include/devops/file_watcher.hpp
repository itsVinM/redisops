#pragma once
#include <string>
#include <vector>
#include <functional>
#include <thread>
#include <atomic>

namespace devops {

struct FileEvent {
    std::string path;
    uint32_t mask;  // IN_CREATE, IN_MODIFY, IN_DELETE, etc.
};

using FileWatcherCallback = std::function<void(const FileEvent&)>;

class FileWatcher {
public:
    FileWatcher();
    ~FileWatcher();

    bool watch(const std::string& path, uint32_t mask);
    void on_change(FileWatcherCallback cb);
    void start();
    void stop();
    bool is_running() const;

private:
    void poll_loop();

    int inotify_fd_ = -1;
    std::vector<int> watch_fds_;
    FileWatcherCallback callback_;
    std::thread thread_;
    std::atomic<bool> running_{false};
};

} // namespace devops
