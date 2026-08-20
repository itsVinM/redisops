#pragma once
#include <iostream>
#include <string>
#include <vector>
#include <functional>
#include <cstring>

namespace test {

struct TestResult {
    std::string name;
    bool passed;
    std::string error;
};

static std::vector<TestResult>& results() {
    static std::vector<TestResult> r;
    return r;
}

static int& failures() {
    static int f = 0;
    return f;
}

#define TEST(name) \
    static void test_##name(); \
    static struct Reg_##name { \
        Reg_##name() { test::results().push_back({#name, false, ""}); } \
    } reg_##name; \
    static void test_##name()

#define ASSERT_TRUE(expr) \
    do { \
        if (!(expr)) { \
            test::results().back().error = std::string("ASSERT_TRUE failed: ") + #expr + " at " + __FILE__ + ":" + std::to_string(__LINE__); \
            return; \
        } \
    } while(0)

#define ASSERT_FALSE(expr) ASSERT_TRUE(!(expr))

#define ASSERT_EQ(a, b) \
    do { \
        if ((a) != (b)) { \
            test::results().back().error = std::string("ASSERT_EQ failed: ") + #a + " != " + #b + " at " + __FILE__ + ":" + std::to_string(__LINE__); \
            return; \
        } \
    } while(0)

#define ASSERT_NEQ(a, b) \
    do { \
        if ((a) == (b)) { \
            test::results().back().error = std::string("ASSERT_NEQ failed: ") + #a + " == " + #b + " at " + __FILE__ + ":" + std::to_string(__LINE__); \
            return; \
        } \
    } while(0)

#define ASSERT_STREQ(a, b) \
    do { \
        if (std::string(a) != std::string(b)) { \
            test::results().back().error = std::string("ASSERT_STREQ failed: \"") + (a) + "\" != \"" + (b) + "\" at " + __FILE__ + ":" + std::to_string(__LINE__); \
            return; \
        } \
    } while(0)

inline int run_all() {
    int total = 0, passed = 0, failed = 0;
    for (auto& t : results()) {
        total++;
        // Re-run the test function by looking it up
        // We stored results but need to actually run
    }
    // Actually we need to run the tests. Let's use a different approach.
    return failed;
}

} // namespace test
