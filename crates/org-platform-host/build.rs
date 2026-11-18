//! Compiles the recovery presence bridge (ADR-0011) when the build target is macOS.
//!
//! `src/presence/bridge.m` is the Objective-C translation unit behind
//! `org_platform_host::presence` on macOS. Its bytes are pinned below and compared
//! before the compiler reads the file. A build script runs on the build machine, so it
//! reads the target from `CARGO_CFG_TARGET_OS`; it emits no `cfg`. For every other
//! target it compiles and links nothing.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos") {
        println!("cargo::rerun-if-changed=src/presence/bridge.m");
        // Complete reviewed byte pin before the compiler reads this fixed input.
        // The fixture test (`tests/recovery_presence.rs`) compares the same bytes again.
        const BRIDGE: &[u8] = br####"// REC-DEL-1 fixed SDK adapter. No Rust pointer is retained by an SDK callback.
#import <Foundation/Foundation.h>
#import <LocalAuthentication/LocalAuthentication.h>
#include <Security/AuthSession.h>
#include <SystemConfiguration/SystemConfiguration.h>
#include <bsm/libbsm.h>
#include <libproc.h>
#include <mach/mach.h>
#include <sys/param.h>
#include <sys/proc.h>
#include <sys/proc_info.h>
#include <unistd.h>

_Static_assert(sizeof(audit_token_t) == 32, "unsupported audit token ABI");

static bool local_identity(pid_t pid, uid_t uid, au_asid_t asid) {
    if (pid <= 0 || uid == 0 || uid != geteuid() || asid <= 0) return false;
    return true;
}

static bool local_session(au_asid_t asid) {
    SecuritySessionId actual = 0, own = 0;
    SessionAttributeBits flags = 0, own_flags = 0;
    if (SessionGetInfo((SecuritySessionId)asid, &actual, &flags) != errSecSuccess ||
        SessionGetInfo(callerSecuritySession, &own, &own_flags) != errSecSuccess ||
        actual != (SecuritySessionId)asid || actual != own ||
        !(flags & sessionHasGraphicAccess) || !(own_flags & sessionHasGraphicAccess) ||
        (flags & (sessionIsRemote | sessionIsRoot)) ||
        (own_flags & (sessionIsRemote | sessionIsRoot))) return false;
    return true;
}

static bool local_console_user(uid_t uid) {
    uid_t console_uid = 0;
    CFStringRef user = SCDynamicStoreCopyConsoleUser(NULL, &console_uid, NULL);
    if (!user) return false;
    CFRelease(user);
    if (console_uid == 0 || console_uid != uid) return false;
    return true;
}

static bool local_foreground_process(pid_t pid, uid_t uid) {
    struct proc_bsdinfo info = {0};
    int size = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, sizeof(info));
    return size == (int)sizeof(info) && info.pbi_pid == (uint32_t)pid &&
        info.pbi_uid == uid && info.pbi_ruid == uid && info.pbi_status != SZOMB &&
        info.e_tdev != (uint32_t)NODEV && info.pbi_pgid != 0 &&
        info.pbi_pgid == info.e_tpgid;
}

static bool local_token(audit_token_t token) {
    pid_t pid = audit_token_to_pid(token);
    uid_t uid = audit_token_to_euid(token);
    au_asid_t asid = audit_token_to_asid(token);
    if (!local_identity(pid, uid, asid)) return false;
    if (!local_session(asid)) return false;
    if (!local_console_user(uid)) return false;
    return local_foreground_process(pid, uid);
}

int ag_recovery_local(const unsigned char *bytes, size_t length) {
    @autoreleasepool { @try {
        if (!bytes || length != sizeof(audit_token_t)) return 0;
        audit_token_t token;
        memcpy(&token, bytes, sizeof(token));
        return local_token(token) ? 1 : 0;
    } @catch (NSException *exception) { (void)exception; return 0; } }
}
int ag_recovery_current(void) {
    @autoreleasepool { @try {
        audit_token_t token = {{0}};
        mach_msg_type_number_t count = TASK_AUDIT_TOKEN_COUNT;
        if (task_info(mach_task_self(), TASK_AUDIT_TOKEN, (task_info_t)&token, &count) != KERN_SUCCESS ||
            count != TASK_AUDIT_TOKEN_COUNT) return 0;
        return local_token(token) ? 1 : 0;
    } @catch (NSException *exception) { (void)exception; return 0; } }
}

@interface AGRecoveryPresenceState : NSObject {
@public
    LAContext *context;
    bool finishing;
    bool cancelled;
    int terminal; // 0 pending, 1 authenticated, 2 denied/cancelled
}
@end
@implementation AGRecoveryPresenceState
@end

// ARC owns the heap state both through the returned retained handle and any
// late SDK block. Terminal transitions are single-assignment under the lock.
static void finish(AGRecoveryPresenceState *state, int result) {
    if (!state) return;
    @try {
        LAContext *context = nil;
        @synchronized(state) {
            if (result != 1) state->cancelled = true;
            if (state->terminal != 0) return;
            if (state->finishing) { state->cancelled = true; return; }
            state->finishing = true;
            context = state->context;
            state->context = nil; // break context/block/state ownership
        }
        bool success = result == 1;
        @try { [context invalidate]; }
        @catch (NSException *exception) { (void)exception; success = false; }
        @synchronized(state) {
            if (state->terminal == 0)
                state->terminal = success && !state->cancelled ? 1 : 2;
        }
    } @catch (NSException *exception) {
        (void)exception;
        // Never let an Objective-C exception cross the C ABI or SDK callback.
        // A failed lock/query cannot publish success; Rust also bounds the wait.
        @try { @synchronized(state) { if (state->terminal == 0) state->terminal = 2; } }
        @catch (NSException *ignored) { (void)ignored; }
    }
}
void *ag_recovery_begin(void) {
    @autoreleasepool {
        AGRecoveryPresenceState *state = nil;
        @try {
        state = [AGRecoveryPresenceState new];
        if (!state) return NULL;
        state->context = [LAContext new];
        if (!state->context) return NULL;
        state->context.touchIDAuthenticationAllowableReuseDuration = 0;
        NSError *error = nil;
        if (![state->context canEvaluatePolicy:LAPolicyDeviceOwnerAuthentication error:&error] || error) {
            finish(state, 2); return NULL;
        }
        [state->context evaluatePolicy:LAPolicyDeviceOwnerAuthentication
            localizedReason:@"Show the recovery code and record it as stored off this Mac."
            reply:^(BOOL success, NSError *reply_error) {
                @try { finish(state, success && !reply_error ? 1 : 2); }
                @catch (NSException *exception) { (void)exception; finish(state, 2); }
            }];
        return (__bridge_retained void *)state;
    } @catch (NSException *exception) { (void)exception; finish(state, 2); return NULL; } }
}
int ag_recovery_poll(void *handle) {
    @autoreleasepool { @try {
        if (!handle) return 2;
        AGRecoveryPresenceState *state = (__bridge AGRecoveryPresenceState *)handle;
        @synchronized(state) { return state->terminal; }
    } @catch (NSException *exception) { (void)exception; return 2; } }
}
void ag_recovery_free(void *handle) {
    @autoreleasepool { @try {
        if (handle) {
            AGRecoveryPresenceState *state = (__bridge_transfer AGRecoveryPresenceState *)handle;
            finish(state, 2); // invalidate even when Rust timed out before the SDK reply
        }
    } @catch (NSException *exception) { (void)exception; } }
}
"####;
        assert_eq!(
            std::fs::read("src/presence/bridge.m").expect("recovery bridge input"),
            BRIDGE,
            "unreviewed recovery bridge bytes"
        );
        cc::Build::new()
            .file("src/presence/bridge.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .warnings_into_errors(true)
            .compile("agentic_org_recovery_presence");
        for framework in [
            "Foundation",
            "LocalAuthentication",
            "Security",
            "SystemConfiguration",
        ] {
            println!("cargo::rustc-link-lib=framework={framework}");
        }
        println!("cargo::rustc-link-lib=bsm");
    }
}
