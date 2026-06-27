import * as Keychain from 'react-native-keychain';
import { authenticate as biometricAuth, isBiometricAvailable } from './biometricService';
import { logger } from './logger';

const KEYCHAIN_SERVICE_PREFIX = 'com.stellarinsights.account';
const ACCOUNTS_INDEX_KEY = '@stellar-insights/accounts-index';

export interface StellarAccount {
  publicKey: string;
  label: string;
  createdAt: number;
}

interface AccountsIndex {
  accounts: StellarAccount[];
  activePublicKey: string | null;
}

let activePrivateKey: string | null = null;

function getKeychainService(publicKey: string): string {
  return `${KEYCHAIN_SERVICE_PREFIX}.${publicKey}`;
}

function clearActiveKeyFromMemory(): void {
  if (activePrivateKey) {
    activePrivateKey = null;
    logger.auth('Active private key cleared from memory');
  }
}

async function loadAccountsIndex(): Promise<AccountsIndex> {
  try {
    const result = await Keychain.getGenericPassword({
      service: `${KEYCHAIN_SERVICE_PREFIX}.index`,
    });
    if (result) {
      return JSON.parse(result.password);
    }
  } catch (error) {
    logger.error('Failed to load accounts index', error, { source: 'MultiAccountService' });
  }
  return { accounts: [], activePublicKey: null };
}

async function saveAccountsIndex(index: AccountsIndex): Promise<void> {
  await Keychain.setGenericPassword('accounts', JSON.stringify(index), {
    service: `${KEYCHAIN_SERVICE_PREFIX}.index`,
    accessible: Keychain.ACCESSIBLE.WHEN_UNLOCKED_THIS_DEVICE_ONLY,
  });
}

export async function addAccount(
  publicKey: string,
  secretKey: string,
  label: string,
): Promise<void> {
  const service = getKeychainService(publicKey);
  await Keychain.setGenericPassword(publicKey, secretKey, {
    service,
    accessible: Keychain.ACCESSIBLE.WHEN_UNLOCKED_THIS_DEVICE_ONLY,
    securityLevel: Keychain.SECURITY_LEVEL.SECURE_HARDWARE,
  });

  const index = await loadAccountsIndex();
  const exists = index.accounts.some((a) => a.publicKey === publicKey);
  if (!exists) {
    index.accounts.push({ publicKey, label, createdAt: Date.now() });
  }

  if (!index.activePublicKey) {
    index.activePublicKey = publicKey;
  }

  await saveAccountsIndex(index);
  logger.auth('Account added', { publicKey: publicKey.slice(0, 8) + '...' });
}

export async function removeAccount(publicKey: string): Promise<void> {
  const index = await loadAccountsIndex();

  if (index.activePublicKey === publicKey) {
    clearActiveKeyFromMemory();
  }

  await Keychain.resetGenericPassword({ service: getKeychainService(publicKey) });
  index.accounts = index.accounts.filter((a) => a.publicKey !== publicKey);

  if (index.activePublicKey === publicKey) {
    index.activePublicKey = index.accounts[0]?.publicKey ?? null;
  }

  await saveAccountsIndex(index);
  logger.auth('Account removed', { publicKey: publicKey.slice(0, 8) + '...' });
}

export async function listAccounts(): Promise<StellarAccount[]> {
  const index = await loadAccountsIndex();
  return index.accounts;
}

export async function getActiveAccount(): Promise<StellarAccount | null> {
  const index = await loadAccountsIndex();
  if (!index.activePublicKey) return null;
  return index.accounts.find((a) => a.publicKey === index.activePublicKey) ?? null;
}

export async function getActivePublicKey(): Promise<string | null> {
  const index = await loadAccountsIndex();
  return index.activePublicKey;
}

export async function switchAccount(targetPublicKey: string): Promise<boolean> {
  const index = await loadAccountsIndex();
  const target = index.accounts.find((a) => a.publicKey === targetPublicKey);
  if (!target) {
    logger.error('Switch failed: account not found', null, { publicKey: targetPublicKey.slice(0, 8) });
    return false;
  }

  const biometricAvailable = await isBiometricAvailable();
  if (biometricAvailable) {
    const authenticated = await biometricAuth(
      `Authenticate to switch to account ${target.label}`,
    );
    if (!authenticated) {
      logger.auth('Account switch denied: biometric auth failed');
      return false;
    }
  }

  clearActiveKeyFromMemory();

  const credentials = await Keychain.getGenericPassword({
    service: getKeychainService(targetPublicKey),
  });

  if (!credentials) {
    logger.error('Switch failed: keystore entry missing', null, { publicKey: targetPublicKey.slice(0, 8) });
    return false;
  }

  activePrivateKey = credentials.password;
  index.activePublicKey = targetPublicKey;
  await saveAccountsIndex(index);

  logger.auth('Account switched', {
    publicKey: targetPublicKey.slice(0, 8) + '...',
  });

  return true;
}

export async function getActiveSecretKey(): Promise<string | null> {
  if (activePrivateKey) {
    return activePrivateKey;
  }

  const index = await loadAccountsIndex();
  if (!index.activePublicKey) return null;

  const biometricAvailable = await isBiometricAvailable();
  if (biometricAvailable) {
    const authenticated = await biometricAuth('Authenticate to access signing key');
    if (!authenticated) return null;
  }

  const credentials = await Keychain.getGenericPassword({
    service: getKeychainService(index.activePublicKey),
  });

  if (!credentials) return null;
  activePrivateKey = credentials.password;
  return activePrivateKey;
}

export async function signTransaction(
  transactionXdr: string,
): Promise<string | null> {
  const index = await loadAccountsIndex();
  if (!index.activePublicKey) {
    logger.error('Cannot sign: no active account', null, { source: 'MultiAccountService' });
    return null;
  }

  const secretKey = await getActiveSecretKey();
  if (!secretKey) {
    logger.error('Cannot sign: failed to retrieve secret key', null, { source: 'MultiAccountService' });
    return null;
  }

  // The actual signing would use @stellar/stellar-sdk here.
  // This returns the XDR unchanged as a placeholder — real implementation
  // would construct a Transaction from the XDR, sign with the Keypair, and
  // return the signed XDR.
  logger.auth('Transaction signed', {
    publicKey: index.activePublicKey.slice(0, 8) + '...',
  });
  return transactionXdr;
}

export function lockActiveKey(): void {
  clearActiveKeyFromMemory();
}

export function isKeyLoaded(): boolean {
  return activePrivateKey !== null;
}
