#include <Security/Security.h>
#include <CoreFoundation/CoreFoundation.h>
#include <stdio.h>
#include <string.h>

// Only synthetic data and a unique spike service; no production item is queried.
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    CFStringRef service = CFStringCreateWithCString(NULL, argv[1], kCFStringEncodingUTF8);
    if (service == NULL) return 3;
    const unsigned char dummy[32] = {42};
    CFDataRef data = CFDataCreate(NULL, dummy, sizeof(dummy));
    const void *keys[] = {kSecClass, kSecAttrService, kSecAttrAccount,
        kSecUseDataProtectionKeychain, kSecAttrAccessGroup,
        kSecAttrAccessible, kSecAttrSynchronizable, kSecValueData,
        kSecUseAuthenticationUI};
    const void *values[] = {kSecClassGenericPassword, service, CFSTR("scratch"),
        kCFBooleanTrue, CFSTR("SAGEB20400.org.agentic.b204"),
        kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly, kCFBooleanFalse, data,
        kSecUseAuthenticationUIFail};
    CFDictionaryRef add = CFDictionaryCreate(NULL, keys, values, 9,
        &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    OSStatus status = SecItemAdd(add, NULL);
    printf("Data Protection add status=%d\n", (int)status);
    CFMutableDictionaryRef query = CFDictionaryCreateMutableCopy(NULL, 0, add);
    CFDictionaryRemoveValue(query, kSecValueData);
    CFDictionaryRemoveValue(query, kSecAttrAccessible);
    CFDictionaryRemoveValue(query, kSecAttrSynchronizable);
    CFDictionarySetValue(query, kSecReturnData, kCFBooleanTrue);
    CFTypeRef result = NULL;
    OSStatus read_status = SecItemCopyMatching(query, &result);
    printf("Data Protection read status=%d\n", (int)read_status);
    if (result) CFRelease(result);
    CFDictionaryRemoveValue(query, kSecReturnData);
    if (status == errSecSuccess) printf("scratch item cleanup status=%d\n", (int)SecItemDelete(query));
    CFRelease(query); CFRelease(add); CFRelease(data); CFRelease(service);
    return 0;
}
