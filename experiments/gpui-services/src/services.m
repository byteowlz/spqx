#import <AppKit/AppKit.h>

typedef int (*SPQXSelectionCallback)(const unsigned char *, size_t);
static SPQXSelectionCallback selectionCallback;

@interface SPQXServiceProbe : NSObject
- (void)readSelection:(NSPasteboard *)pasteboard
            userData:(NSString *)userData
               error:(NSString **)error;
@end

@implementation SPQXServiceProbe
- (void)readSelection:(NSPasteboard *)pasteboard
            userData:(NSString *)userData
               error:(NSString **)error {
    (void)userData;
    NSString *text = [pasteboard stringForType:NSPasteboardTypeString];
    BOOL accepted = text && selectionCallback &&
        selectionCallback((const unsigned char *)text.UTF8String,
                          [text lengthOfBytesUsingEncoding:NSUTF8StringEncoding]);
    if (!accepted && error) {
        // Never put selection text into an error, log or process argument.
        *error = @"SPQX could not queue this selection. Try a shorter text when idle.";
    }
}
@end

static SPQXServiceProbe *provider;

void spqx_probe_register(SPQXSelectionCallback callback) {
    selectionCallback = callback;
    provider = [SPQXServiceProbe new];
    [NSApp setServicesProvider:provider];
}

// Exercises the real ObjC handler with an OWNED pasteboard, never generalPasteboard.
// This is not proof of OS Services-menu routing from another application.
int spqx_probe_exercise(void) {
    NSPasteboard *pasteboard = [NSPasteboard pasteboardWithUniqueName];
    [pasteboard declareTypes:@[NSPasteboardTypeString] owner:nil];
    [pasteboard setString:@"Synthetic selection — Grüße" forType:NSPasteboardTypeString];
    NSString *error = nil;
    [provider readSelection:pasteboard userData:nil error:&error];
    [pasteboard releaseGlobally];
    return error == nil;
}
