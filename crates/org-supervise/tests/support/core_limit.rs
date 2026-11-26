// Test-only startup hook, before libtest creates any threads. Lowering hard0 is
// irreversible for this process and its children; never restore/change it in a test.
// Each integration test binary and the library's unit-test binary include this module.
org_platform::process::testing::disable_core_dumps_before_main!();
