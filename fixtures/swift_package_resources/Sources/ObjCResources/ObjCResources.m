#import "Internal.h"
#import "ObjCResources.h"

NSString *ObjCResourceFileName(void) {
    return @"objc-message";
}

NSString *ObjCResourceMessage(void) {
    NSURL *url = [SWIFTPM_MODULE_BUNDLE URLForResource:ObjCResourceFileName() withExtension:@"txt"];
    return [[NSString stringWithContentsOfURL:url encoding:NSUTF8StringEncoding error:nil] stringByTrimmingCharactersInSet:NSCharacterSet.newlineCharacterSet];
}
