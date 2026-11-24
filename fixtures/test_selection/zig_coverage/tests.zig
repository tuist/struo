const std = @import("std");
const product = @import("product.zig");
const unused = @import("unused.zig");

test "atomic invocation" {
    if (std.testing.environ.getPosix("STRUO_FIXTURE_SKIP") != null) return error.SkipZigTest;
    if (std.testing.environ.getPosix("STRUO_FIXTURE_FAIL") != null) return error.FixtureFailure;
    if (std.testing.environ.getPosix("STRUO_FIXTURE_CHILD") != null) {
        const child = try std.process.run(std.testing.allocator, std.testing.io, .{ .argv = &.{"true"} });
        std.testing.allocator.free(child.stdout);
        std.testing.allocator.free(child.stderr);
    }
    if (std.testing.environ.getPosix("STRUO_FIXTURE_UNUSED") != null) {
        try std.testing.expectEqual(@as(u32, 42), unused.value());
    }
    try std.testing.expectEqual(@as(u32, 17), product.alpha());
    try std.testing.expectEqual(@as(u32, 31), product.beta());
}
