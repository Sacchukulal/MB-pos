import '@testing-library/jest-dom/vitest';
import { configure } from '@testing-library/react';

/** A test that fails because the machine was busy is a test nobody trusts. */
configure({ asyncUtilTimeout: 5_000 });
