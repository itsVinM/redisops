#include <devops/file_watcher.hpp>
#include <iostream>
#include <cstring>

#ifdef __linux__
#include <sys/inotify.h>
#include <unistd.h>
#include <poll.h>
#endif

namespace devops {

FileWatcher::FileWatcher() = default;

FileWatcher::~FileWatcher() { stop(); }

bool FileWatcher::watch(const std::string& path, uint32_t mask) {
#ifdef __linux__
    if (inotify_fd_ < 0) {
        inotify_fd_ = inotify_init1(IN_NONBLOCK);
        if (inotify_fd_ < 0) return false;
    }

    int wd = inotify_add_watch(inotify_fd_, path.c_str(), mask);
    if (wd < 0) return false;

    watch_fds_.push_back(wd);
    return true;
#else
    return false;
#endif
}

void FileWatcher::on_change(FileWatcherCallback cb) {
    callback_ = std::move(cb);
}

void FileWatcher::start() {
    if (running_) return;
    running_ = true;
    thread_ = std::thread(&FileWatcher::poll_loop, this);
}

void FileWatcher::stop() {
    running_ = false;
    if (thread_.joinable()) thread_.join();
#ifdef __linux__
    if (inotify_fd_ >= 0) {
        for (int wd : watch_fds_) {
            inotify_rm_watch(inotify_fd_, wd);
        }
        close(inotify_fd_);
        inotify_fd_ = -1;
    }
#endif
}

bool FileWatcher::is_running() const { return running_; }

void FileWatcher::poll_loop() {
#ifdef __linux__
    constexpr size_t EVENT_SIZE = sizeof(inotify_event);
    constexpr size_t BUF_LEN = 1024 * (EVENT_SIZE + 16);
    char buf[BUF_LEN];

    while (running_) {
        pollfd pfd{inotify_fd_, POLLIN, 0};
        int ret = poll(&pfd, 1, 500);  // 500ms timeout

        if (ret > 0 && (pfd.revents & POLLIN)) {
            ssize_t len = read(inotify_fd_, buf, BUF_LEN);
            if (len <= 0) continue;

            const char* ptr = buf;
            while (ptr < buf + len) {
                const auto* event = reinterpret_cast<const inotify_event*>(ptr);

                if (callback_) {
                    FileEvent fe;
                    fe.path = event->len > 0 ? event->name : "";
                    fe.mask = event->mask;
                    callback_(fe);
                }

                ptr += EVENT_SIZE + event->len;
            }
        }
    }
#endif
}

} // namespace devops
