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
)

target_include_directories(sandbox PRIVATE include/)
