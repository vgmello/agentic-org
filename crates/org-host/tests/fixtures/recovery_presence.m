// Disposable isolated SDK substitute. Not application code or a trust override.
// The production translation unit is compiled separately without modification.
#import <Foundation/Foundation.h>
#import <LocalAuthentication/LocalAuthentication.h>
#import <objc/runtime.h>
#import <dispatch/dispatch.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern void *ag_recovery_begin(void);
extern int ag_recovery_poll(void *handle);
extern void ag_recovery_free(void *handle);

typedef void (^Reply)(BOOL, NSError *);
static NSString *scenario;
static Reply heldReply;
static __weak id weakState;
static __weak id weakContext;
static NSUInteger beginCount, newCount, canCount, evaluateCount, invalidateCount;
static NSUInteger deallocCount, reentrantCount, checks;
static BOOL reuseWasZero;

static void require(BOOL ok, const char *message) {
    checks++;
    if (!ok) {
        fprintf(stderr, "FAIL scenario=%s check=%s\n", scenario.UTF8String, message);
        fflush(NULL);
        _Exit(70);
    }
}
static BOOL is(NSString *name) { return [scenario isEqualToString:name]; }
static NSError *denialError(void) {
    return [NSError errorWithDomain:@"DisposableSDKProbe" code:1 userInfo:nil];
}

// NSObject, intentionally NOT an LAContext subclass: no real SDK initializer,
// deallocator or method implementation can run through the substitute instance.
@interface DisposableContext : NSObject
@property(nonatomic, copy) Reply reply;
@end
@implementation DisposableContext
- (void)setTouchIDAuthenticationAllowableReuseDuration:(NSTimeInterval)value {
    require(value == 0, "reuse duration must be zero");
    reuseWasZero = YES;
}
- (BOOL)canEvaluatePolicy:(LAPolicy)policy error:(NSError *__autoreleasing *)error {
    canCount++;
    require(policy == LAPolicyDeviceOwnerAuthentication, "correct canEvaluate policy");
    require(reuseWasZero, "reuse set before canEvaluate");
    if (error) *error = is(@"can_yes_with_error") ? denialError() : nil;
    return !is(@"can_denied");
}
- (void)evaluatePolicy:(LAPolicy)policy localizedReason:(NSString *)reason reply:(Reply)reply {
    evaluateCount++;
    require(policy == LAPolicyDeviceOwnerAuthentication, "correct evaluate policy");
    require([reason isEqualToString:@"Show the recovery code and record it as stored off this Mac."],
            "exact reason preserved");
    require(reply != nil, "real product reply block supplied");
    self.reply = reply; // Genuine ARC copy of the block from the product unit.
    heldReply = [reply copy]; // Simulate SDK ownership beyond context invalidation.
    require(strstr(object_getClassName(heldReply), "MallocBlock") != NULL,
            "capturing product block lives on heap");
    if (is(@"evaluate_exception")) {
        @throw [NSException exceptionWithName:@"DisposableEvaluateException" reason:nil userInfo:nil];
    }
    if (is(@"synchronous_success")) heldReply(YES, nil);
    if (is(@"synchronous_denial")) heldReply(NO, denialError());
}
- (void)invalidate {
    invalidateCount++;
    if (is(@"reentrant_success") || is(@"reentrant_denial")) {
        require(reentrantCount == 0, "invalidation does not recur");
        reentrantCount++;
        heldReply(is(@"reentrant_success"), nil);
    }
    if (is(@"invalidate_exception_success") || is(@"invalidate_exception_cancel")) {
        @throw [NSException exceptionWithName:@"DisposableInvalidateException" reason:nil userInfo:nil];
    }
}
- (void)dealloc { deallocCount++; }
@end

// +new's C IMP must return +1 under ARC, matching the Objective-C method family.
static id __attribute__((ns_returns_retained)) substituteNew(id receiver, SEL command) {
    (void)receiver; (void)command;
    newCount++;
    DisposableContext *context = [DisposableContext new];
    weakContext = context;
    return context;
}
static void blocked(const char *method) {
    fprintf(stderr, "REAL_SDK_ENTRY_BLOCKED selector=%s begin_count=%lu\n", method,
            (unsigned long)beginCount);
    fflush(NULL);
    _Exit(86);
}
static id __attribute__((ns_returns_retained)) blockedNew(id receiver, SEL command) {
    (void)receiver; blocked(sel_getName(command)); return nil;
}
static BOOL blockedCan(id receiver, SEL command, LAPolicy policy, NSError *__autoreleasing *error) {
    (void)receiver; (void)policy; (void)error; blocked(sel_getName(command)); return NO;
}
static void blockedEvaluate(id receiver, SEL command, LAPolicy policy, NSString *reason, Reply reply) {
    (void)receiver; (void)policy; (void)reason; (void)reply; blocked(sel_getName(command));
}
static void blockedInvalidate(id receiver, SEL command) {
    (void)receiver; blocked(sel_getName(command));
}
static void blockedReuse(id receiver, SEL command, NSTimeInterval value) {
    (void)receiver; (void)value; blocked(sel_getName(command));
}

static Method method(Class cls, SEL selector, BOOL meta) {
    Method result = meta ? class_getClassMethod(cls, selector) : class_getInstanceMethod(cls, selector);
    require(result != NULL, "expected SDK selector exists");
    return result;
}
static void replace(Class cls, SEL selector, IMP imp, BOOL meta) {
    Method old = method(cls, selector, meta);
    Class target = meta ? object_getClass(cls) : cls;
    // Add an inherited method to this class, never mutate NSObject's copy.
    if (!class_addMethod(target, selector, imp, method_getTypeEncoding(old))) {
        method_setImplementation(method(cls, selector, meta), imp);
    }
    require(method_getImplementation(method(cls, selector, meta)) == imp,
            "replacement installed on exact class");
}
static void installInterception(void) {
    Class cls = objc_getClass("LAContext");
    require(cls != Nil, "SDK class linked");
    replace(cls, @selector(evaluatePolicy:localizedReason:reply:), (IMP)blockedEvaluate, NO);
    replace(cls, @selector(canEvaluatePolicy:error:), (IMP)blockedCan, NO);
    replace(cls, @selector(invalidate), (IMP)blockedInvalidate, NO);
    replace(cls, @selector(setTouchIDAuthenticationAllowableReuseDuration:), (IMP)blockedReuse, NO);
    replace(cls, @selector(new), (IMP)substituteNew, YES);
    require(class_getSuperclass(objc_getClass("DisposableContext")) == objc_getClass("NSObject"),
            "substitute has no LAContext superclass");
}
static void verifyInterception(void) {
    Class cls = objc_getClass("LAContext");
    BOOL ok = method_getImplementation(method(cls, @selector(new), YES)) == (IMP)substituteNew &&
        method_getImplementation(method(cls, @selector(evaluatePolicy:localizedReason:reply:), NO)) == (IMP)blockedEvaluate &&
        method_getImplementation(method(cls, @selector(canEvaluatePolicy:error:), NO)) == (IMP)blockedCan &&
        method_getImplementation(method(cls, @selector(invalidate), NO)) == (IMP)blockedInvalidate &&
        method_getImplementation(method(cls, @selector(setTouchIDAuthenticationAllowableReuseDuration:), NO)) == (IMP)blockedReuse;
    if (!ok) {
        fprintf(stderr, "INTERCEPTION_GUARD_REFUSED begin_count=%lu\n", (unsigned long)beginCount);
        fflush(NULL);
        _Exit(87);
    }
}
static void *begin(void) {
    verifyInterception(); // Must pass immediately before every C ABI begin.
    beginCount++;
    return ag_recovery_begin();
}
static void assertTerminal(int expected) {
    // A copied product block owns the object after free; temporarily retain that
    // live weak reference before bridging it. Never dereference a freed handle.
    id liveState = weakState;
    require(liveState != nil, "callback still owns actual product state");
    require(ag_recovery_poll((__bridge void *)liveState) == expected, "expected terminal value");
}
static void releaseCallbackAndAssertCollected(void) {
    @autoreleasepool { heldReply = nil; }
    require(weakState == nil, "product state released after handle and callback");
    require(weakContext == nil && deallocCount == 1, "context released exactly once");
}
static void replyOnBackgroundThread(BOOL success, NSError *error) {
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^{
        @autoreleasepool { heldReply(success, error); }
        dispatch_semaphore_signal(done);
    });
    require(dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC)) == 0,
            "background callback completes without deadlock");
}

static void run(void) {
    require(ag_recovery_poll(NULL) == 2, "null poll denied");
    ag_recovery_free(NULL);
    void *handle = NULL;
    @autoreleasepool { handle = begin(); }
    require(beginCount == 1 && newCount == 1 && canCount == 1, "one substitute context per begin");
    if (is(@"can_denied") || is(@"can_yes_with_error")) {
        require(handle == NULL && evaluateCount == 0 && heldReply == nil, "preflight denial has no evaluate");
        require(invalidateCount == 1, "preflight denial invalidates");
        require(weakContext == nil && deallocCount == 1, "denied context collected");
        return;
    }
    if (is(@"evaluate_exception")) {
        require(handle == NULL && evaluateCount == 1, "evaluate exception caught by C ABI begin");
        // The substitute retained the block before throwing; it still keeps
        // product state alive. Observe only its captured state through a later
        // callback, and ensure the context was detached even on this failure.
        require(invalidateCount == 1 && weakContext == nil, "exception breaks context cycle");
        heldReply(YES, nil);
        heldReply = nil;
        require(deallocCount == 1, "exception context deallocated");
        return;
    }
    require(handle != NULL && evaluateCount == 1, "begin returns retained real product state");
    weakState = (__bridge id)handle;
    require(weakState != nil && heldReply != nil, "state and block outlive begin autoreleasepool");
    BOOL synchronous = is(@"synchronous_success") || is(@"synchronous_denial");
    require(ag_recovery_poll(handle) == (synchronous ? (is(@"synchronous_success") ? 1 : 2) : 0),
            "initial state reflects synchronous or pending reply");
    if (synchronous) {
        heldReply(YES, nil);
        require(ag_recovery_poll(handle) == (is(@"synchronous_success") ? 1 : 2), "synchronous terminal stable");
        ag_recovery_free(handle);
    } else if (is(@"cancel_late_success") || is(@"cancel_late_denial") || is(@"invalidate_exception_cancel")) {
        require(weakContext != nil, "pending context retained by state");
        ag_recovery_free(handle);
        handle = NULL; // Do not use or free the consumed retained handle again.
        assertTerminal(2);
        require(weakContext == nil, "free detaches context before late reply");
        replyOnBackgroundThread(!is(@"cancel_late_denial"), nil);
        assertTerminal(2);
        replyOnBackgroundThread(YES, nil);
        assertTerminal(2);
    } else {
        BOOL denied = is(@"denied_then_success");
        BOOL withError = is(@"yes_with_error");
        heldReply(!denied, withError ? denialError() : nil);
        int expected = (denied || withError || is(@"reentrant_success") || is(@"reentrant_denial") ||
                        is(@"invalidate_exception_success")) ? 2 : 1;
        require(ag_recovery_poll(handle) == expected, "first callback terminal");
        require(weakContext == nil, "callback breaks context ownership cycle");
        replyOnBackgroundThread(YES, nil);
        require(ag_recovery_poll(handle) == expected, "duplicate success cannot upgrade denial");
        replyOnBackgroundThread(NO, denialError());
        require(ag_recovery_poll(handle) == expected, "duplicate denial cannot replace success");
        ag_recovery_free(handle);
    }
    require(invalidateCount == 1, "exactly one invalidation despite duplicate callbacks/free");
    if (is(@"reentrant_success") || is(@"reentrant_denial")) require(reentrantCount == 1, "reentrant callback exercised");
    releaseCallbackAndAssertCollected();
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        if (argc != 2) return 64;
        scenario = [NSString stringWithUTF8String:argv[1]];
        installInterception();
        if (is(@"interception_negative_control")) {
            replace(objc_getClass("LAContext"), @selector(new), (IMP)blockedNew, YES);
            verifyInterception(); // Expected exit 87, before beginCount advances.
            return 71;
        }
        verifyInterception();
        if (is(@"sentinel_negative_control")) {
            IMP imp = method_getImplementation(method(objc_getClass("LAContext"),
                @selector(evaluatePolicy:localizedReason:reply:), NO));
            ((void (*)(id, SEL, LAPolicy, NSString *, Reply))imp)(nil,
                @selector(evaluatePolicy:localizedReason:reply:), LAPolicyDeviceOwnerAuthentication, nil, nil);
            return 72; // Expected exit 86; no SDK object or original method used.
        }
        @try { run(); }
        @catch (NSException *exception) {
            fprintf(stderr, "UNEXPECTED_EXCEPTION %s\n", exception.name.UTF8String);
            return 73;
        }
        printf("PASS scenario=%s checks=%lu begins=%lu contexts=%lu evaluate=%lu invalidate=%lu dealloc=%lu reentrant=%lu heap_block=verified\n",
               scenario.UTF8String, (unsigned long)checks, (unsigned long)beginCount,
               (unsigned long)newCount, (unsigned long)evaluateCount, (unsigned long)invalidateCount,
               (unsigned long)deallocCount, (unsigned long)reentrantCount);
        return 0;
    }
}
