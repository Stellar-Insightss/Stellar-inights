import { useState, useEffect, useCallback } from 'react';
import {
  StellarAccount,
  listAccounts,
  getActiveAccount,
  switchAccount,
  addAccount,
  removeAccount,
  lockActiveKey,
  isKeyLoaded,
} from '../services/MultiAccountService';

interface UseMultiAccountResult {
  accounts: StellarAccount[];
  activeAccount: StellarAccount | null;
  isLoading: boolean;
  isSwitching: boolean;
  isKeyDecrypted: boolean;
  switchTo: (publicKey: string) => Promise<boolean>;
  addNewAccount: (publicKey: string, secretKey: string, label: string) => Promise<void>;
  removeExistingAccount: (publicKey: string) => Promise<void>;
  lock: () => void;
  refresh: () => Promise<void>;
}

export function useMultiAccount(): UseMultiAccountResult {
  const [accounts, setAccounts] = useState<StellarAccount[]>([]);
  const [activeAccount, setActiveAccount] = useState<StellarAccount | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isSwitching, setIsSwitching] = useState(false);
  const [isKeyDecrypted, setIsKeyDecrypted] = useState(false);

  const refresh = useCallback(async () => {
    setIsLoading(true);
    try {
      const [accts, active] = await Promise.all([listAccounts(), getActiveAccount()]);
      setAccounts(accts);
      setActiveAccount(active);
      setIsKeyDecrypted(isKeyLoaded());
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const switchTo = useCallback(
    async (publicKey: string): Promise<boolean> => {
      setIsSwitching(true);
      try {
        const success = await switchAccount(publicKey);
        if (success) {
          await refresh();
        }
        return success;
      } finally {
        setIsSwitching(false);
      }
    },
    [refresh],
  );

  const addNewAccount = useCallback(
    async (publicKey: string, secretKey: string, label: string) => {
      await addAccount(publicKey, secretKey, label);
      await refresh();
    },
    [refresh],
  );

  const removeExistingAccount = useCallback(
    async (publicKey: string) => {
      await removeAccount(publicKey);
      await refresh();
    },
    [refresh],
  );

  const lock = useCallback(() => {
    lockActiveKey();
    setIsKeyDecrypted(false);
  }, []);

  return {
    accounts,
    activeAccount,
    isLoading,
    isSwitching,
    isKeyDecrypted,
    switchTo,
    addNewAccount,
    removeExistingAccount,
    lock,
    refresh,
  };
}
