import '@testing-library/jest-dom/vitest';
import { configure } from '@testing-library/react';

/**
 * **P30: a test that fails because the machine was busy is a test nobody
 * trusts.**
 *
 * Testing Library's default `waitFor` timeout is one second. That is plenty on
 * an idle machine and not plenty on the reference machine (i3 / 4 GB) or on
 * any machine that happens to be compiling Rust in another window — which is
 * exactly when this suite runs. Two kitchen tests failed intermittently under
 * that load and passed every time alone.
 *
 * Five seconds costs nothing when a test passes (it waits for the condition,
 * not for the timeout) and removes a class of failure that teaches people to
 * re-run the suite instead of reading it.
 */
configure({ asyncUtilTimeout: 5_000 });
