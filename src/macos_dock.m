#import <AppKit/AppKit.h>
#include <stdio.h>

bool cah_print_bundle_diagnostics(void) {
    @autoreleasepool {
        NSBundle *bundle = [NSBundle mainBundle];
        NSString *identifier = bundle.bundleIdentifier;
        NSString *executable = [bundle objectForInfoDictionaryKey:@"CFBundleExecutable"];
        printf("bundle_id=%s\n", identifier.UTF8String ?: "(missing)");
        printf("bundle_executable=%s\n", executable.UTF8String ?: "(missing)");
        return identifier.length > 0 && [executable isEqualToString:@"codex-account-hub"];
    }
}

static NSWindow *cah_find_main_window(void) {
    for (NSWindow *window in NSApp.windows) {
        if ([window.title isEqualToString:@"Codex Account Hub"]) {
            return window;
        }
    }

    // The title can be unavailable briefly while eframe is creating the window.
    // The account panel has a fixed size, unlike the status-item helper windows.
    for (NSWindow *window in NSApp.windows) {
        NSSize size = window.frame.size;
        if (size.width >= 398.0 && size.width <= 402.0 &&
            size.height >= 598.0 && size.height <= 602.0) {
            return window;
        }
    }

    return nil;
}

void cah_hide_dock_icon(void) {
    [NSApplication sharedApplication];
    [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
}

static bool cah_panel_wants_visible = true;

void cah_hide_main_window(void) {
    // eframe updates and tray-icon's AppKit callbacks both run on the main thread.
    // Apply transitions synchronously; queued hides can otherwise undo a later show.
    NSCAssert([NSThread isMainThread], @"Panel visibility must run on the main thread");
    cah_panel_wants_visible = false;
    [cah_find_main_window() orderOut:nil];
    // eframe unconditionally shows the window after its very first paint. Retain
    // startup hiding, but never let this deferred correction undo a newer show.
    dispatch_async(dispatch_get_main_queue(), ^{
        if (!cah_panel_wants_visible) [cah_find_main_window() orderOut:nil];
    });
}

void cah_show_main_window(void) {
    NSCAssert([NSThread isMainThread], @"Panel visibility must run on the main thread");
    cah_panel_wants_visible = true;
    NSWindow *window = cah_find_main_window();
    if (!window) return;
    window.collectionBehavior |= NSWindowCollectionBehaviorMoveToActiveSpace;
    // Also supports the packaged macOS 13 target without Clang's availability
    // runtime helper (the final executable is linked by Rust).
    [NSApp activateIgnoringOtherApps:YES];
    [window makeKeyAndOrderFront:nil];
}

bool cah_toggle_main_window(void) {
    NSCAssert([NSThread isMainThread], @"Tray callbacks must run on the main thread");
    NSWindow *window = cah_find_main_window();
    if (!window) return false;
    if (window.isVisible && !window.isMiniaturized) {
        cah_hide_main_window();
        return false;
    }

    // Stay in AppKit's logical coordinates, including Retina and monitors left of
    // or above the primary display. Physical tray pixels are not window points.
    NSPoint anchor = [NSEvent mouseLocation];
    NSScreen *screen = window.screen ?: NSScreen.mainScreen;
    for (NSScreen *candidate in NSScreen.screens) {
        if (NSPointInRect(anchor, candidate.frame)) {
            screen = candidate;
            break;
        }
    }
    if (screen) {
        NSRect area = NSInsetRect(screen.visibleFrame, 8.0, 8.0);
        NSSize size = window.frame.size;
        CGFloat x = MAX(NSMinX(area), MIN(anchor.x - size.width / 2.0,
                                         NSMaxX(area) - size.width));
        CGFloat y = MAX(NSMinY(area), MIN(anchor.y - size.height - 8.0,
                                         NSMaxY(area) - size.height));
        [window setFrameOrigin:NSMakePoint(x, y)];
    }
    if (window.isMiniaturized) [window deminiaturize:nil];
    cah_show_main_window();
    return window.isVisible;
}

// Resolve the exact Codex desktop bundle, including installations named ChatGPT.
// NSRunningApplication termination is cooperative; never escalate to forceTerminate.
static NSURL *cah_desktop_url;
static NSString *const cah_desktop_bundle = @"com.openai.codex";

int cah_prepare_desktop_restart(void) {
    @autoreleasepool {
        NSArray<NSRunningApplication *> *apps =
            [NSRunningApplication runningApplicationsWithBundleIdentifier:cah_desktop_bundle];
        cah_desktop_url = apps.firstObject.bundleURL ?: [[NSWorkspace sharedWorkspace]
            URLForApplicationWithBundleIdentifier:cah_desktop_bundle];
        if (!cah_desktop_url) return 1;
        for (NSRunningApplication *app in apps) {
            if (![app terminate]) return 2;
        }
        return 0;
    }
}

bool cah_desktop_is_running(void) {
    @autoreleasepool {
        return [NSRunningApplication runningApplicationsWithBundleIdentifier:cah_desktop_bundle].count > 0;
    }
}

bool cah_launch_desktop(void) {
    @autoreleasepool {
        if (!cah_desktop_url) return false;
        // Launch asynchronously; completion does not imply authentication changed.
        return [[NSWorkspace sharedWorkspace] openURL:cah_desktop_url];
    }
}
