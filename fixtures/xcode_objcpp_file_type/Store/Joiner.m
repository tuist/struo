#import "Joiner.h"
#import "Vendor/separator.h"

#include <sstream>
#include <string>

@implementation Joiner
+ (NSString *)join:(NSArray<NSString *> *)parts {
    std::ostringstream stream;
    for (NSString *part in parts) {
        if (stream.tellp() > 0) {
            stream << store_separator();
        }
        stream << std::string(part.UTF8String);
    }
    return [NSString stringWithUTF8String:stream.str().c_str()];
}
@end
