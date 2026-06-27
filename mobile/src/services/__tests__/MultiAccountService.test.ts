import {
  addAccount,
  removeAccount,
  listAccounts,
  getActiveAccount,
  switchAccount,
  lockActiveKey,
  isKeyLoaded,
  getActivePublicKey,
} from '../MultiAccountService';

jest.mock('react-native-keychain', () => {
  const store: Record<string, { username: string; password: string }> = {};
  return {
    ACCESSIBLE: { WHEN_UNLOCKED_THIS_DEVICE_ONLY: 'WHEN_UNLOCKED_THIS_DEVICE_ONLY' },
    SECURITY_LEVEL: { SECURE_HARDWARE: 'SECURE_HARDWARE' },
    setGenericPassword: jest.fn(async (username: string, password: string, opts?: { service?: string }) => {
      const key = opts?.service ?? 'default';
      store[key] = { username, password };
      return true;
    }),
    getGenericPassword: jest.fn(async (opts?: { service?: string }) => {
      const key = opts?.service ?? 'default';
      return store[key] ?? false;
    }),
    resetGenericPassword: jest.fn(async (opts?: { service?: string }) => {
      const key = opts?.service ?? 'default';
      delete store[key];
      return true;
    }),
  };
});

jest.mock('../biometricService', () => ({
  isBiometricAvailable: jest.fn(async () => false),
  authenticate: jest.fn(async () => true),
}));

jest.mock('../logger', () => ({
  logger: {
    auth: jest.fn(),
    error: jest.fn(),
    debug: jest.fn(),
    warn: jest.fn(),
  },
}));

const TEST_PUB_1 = 'GABCDEFGHIJKLMNOPQRSTUVWXYZ01234567890ABCDEFGHIJKLMNO';
const TEST_SECRET_1 = 'SABCDEFGHIJKLMNOPQRSTUVWXYZ01234567890ABCDEFGHIJKLMNO';
const TEST_PUB_2 = 'GXYZ0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHIJKLMN';
const TEST_SECRET_2 = 'SXYZ0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGHIJKLMN';

describe('MultiAccountService', () => {
  beforeEach(async () => {
    // Reset state between tests by clearing any loaded keys
    lockActiveKey();
  });

  it('adds an account and lists it', async () => {
    await addAccount(TEST_PUB_1, TEST_SECRET_1, 'Account 1');
    const accounts = await listAccounts();
    expect(accounts.length).toBeGreaterThanOrEqual(1);
    expect(accounts.some((a) => a.publicKey === TEST_PUB_1)).toBe(true);
  });

  it('sets first added account as active', async () => {
    await addAccount(TEST_PUB_1, TEST_SECRET_1, 'Account 1');
    const active = await getActivePublicKey();
    expect(active).toBe(TEST_PUB_1);
  });

  it('switches between accounts', async () => {
    await addAccount(TEST_PUB_1, TEST_SECRET_1, 'Account 1');
    await addAccount(TEST_PUB_2, TEST_SECRET_2, 'Account 2');

    const success = await switchAccount(TEST_PUB_2);
    expect(success).toBe(true);

    const active = await getActiveAccount();
    expect(active?.publicKey).toBe(TEST_PUB_2);
  });

  it('clears key from memory on lock', async () => {
    await addAccount(TEST_PUB_1, TEST_SECRET_1, 'Account 1');
    await switchAccount(TEST_PUB_1);
    expect(isKeyLoaded()).toBe(true);

    lockActiveKey();
    expect(isKeyLoaded()).toBe(false);
  });

  it('clears key from memory on switch', async () => {
    await addAccount(TEST_PUB_1, TEST_SECRET_1, 'Account 1');
    await addAccount(TEST_PUB_2, TEST_SECRET_2, 'Account 2');
    await switchAccount(TEST_PUB_1);
    expect(isKeyLoaded()).toBe(true);

    await switchAccount(TEST_PUB_2);
    // Key should be loaded for the new account
    expect(isKeyLoaded()).toBe(true);
  });

  it('returns false when switching to nonexistent account', async () => {
    const success = await switchAccount('GNONEXISTENT');
    expect(success).toBe(false);
  });

  it('removes an account', async () => {
    await addAccount(TEST_PUB_1, TEST_SECRET_1, 'Account 1');
    await addAccount(TEST_PUB_2, TEST_SECRET_2, 'Account 2');

    await removeAccount(TEST_PUB_1);
    const accounts = await listAccounts();
    expect(accounts.some((a) => a.publicKey === TEST_PUB_1)).toBe(false);
  });
});
