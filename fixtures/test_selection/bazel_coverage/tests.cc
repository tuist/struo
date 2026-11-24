#include <cstdlib>
#include <fstream>
extern int product();
int main() {
    if (std::getenv("STRUO_FIXTURE_SKIP")) {
        std::ofstream(std::getenv("XML_OUTPUT_FILE")) << "<testsuite tests=\"1\" skipped=\"1\"><testcase name=\"skipped\"><skipped/></testcase></testsuite>";
        return 0;
    }
    return std::getenv("STRUO_FIXTURE_FAIL") || product() != 41;
}
