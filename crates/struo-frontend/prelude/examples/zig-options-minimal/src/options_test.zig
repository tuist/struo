const std = @import("std");
const options = @import("build_options");

test "options" {
    try std.testing.expectEqualStrings("hello", options.greeting);
    try std.testing.expect(options.enabled);
    try std.testing.expect(options.port == null);
}
