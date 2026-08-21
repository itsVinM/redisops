cmake_minimum_required(VERSION 3.20)
project(devops-sandbox VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

add_executable(sandbox
    src/main.cpp
    src/redis_client.cpp
    src/sandbox.cpp
    src/cgroup.cpp
    src/seccomp.cpp
    src/metrics.cpp
    src/file_watcher.cpp
    src/health_monitor.cpp
    src/log_aggregator.cpp
    src/netns.cpp
    src/resource_monitor.cpp
    src/process_supervisor.cpp
    src/snapshot.cpp
    src/ipc.cpp
    src/security.cpp
)

target_include_directories(sandbox PRIVATE include/)

# Platform-specific link libraries
if(APPLE)
    target_link_libraries(sandbox PRIVATE pthread)
else()
    target_link_libraries(sandbox PRIVATE pthread rt)
endif()
