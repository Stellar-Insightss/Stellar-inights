import React, { useState } from 'react';
import {
  View,
  Text,
  TouchableOpacity,
  FlatList,
  StyleSheet,
  Alert,
  ActivityIndicator,
  TextInput,
  Modal,
} from 'react-native';
import { useMultiAccount } from '../../hooks/useMultiAccount';
import { StellarAccount } from '../../services/MultiAccountService';

function truncateKey(key: string): string {
  if (key.length <= 12) return key;
  return `${key.slice(0, 6)}...${key.slice(-4)}`;
}

export default function MultiSigApprovalScreen() {
  const {
    accounts,
    activeAccount,
    isLoading,
    isSwitching,
    isKeyDecrypted,
    switchTo,
    addNewAccount,
    removeExistingAccount,
    lock,
  } = useMultiAccount();

  const [showAddModal, setShowAddModal] = useState(false);
  const [newPublicKey, setNewPublicKey] = useState('');
  const [newSecretKey, setNewSecretKey] = useState('');
  const [newLabel, setNewLabel] = useState('');
  const [isAdding, setIsAdding] = useState(false);

  const handleSwitch = async (publicKey: string) => {
    if (publicKey === activeAccount?.publicKey) return;
    const success = await switchTo(publicKey);
    if (!success) {
      Alert.alert(
        'Switch Failed',
        'Biometric authentication is required to switch accounts.',
      );
    }
  };

  const handleRemove = (account: StellarAccount) => {
    Alert.alert(
      'Remove Account',
      `Remove "${account.label}" (${truncateKey(account.publicKey)})? This will delete the stored keys.`,
      [
        { text: 'Cancel', style: 'cancel' },
        {
          text: 'Remove',
          style: 'destructive',
          onPress: () => removeExistingAccount(account.publicKey),
        },
      ],
    );
  };

  const handleAdd = async () => {
    if (!newPublicKey.trim() || !newSecretKey.trim() || !newLabel.trim()) {
      Alert.alert('Error', 'All fields are required.');
      return;
    }

    if (!newPublicKey.startsWith('G') || newPublicKey.length !== 56) {
      Alert.alert('Error', 'Invalid Stellar public key format.');
      return;
    }

    if (!newSecretKey.startsWith('S') || newSecretKey.length !== 56) {
      Alert.alert('Error', 'Invalid Stellar secret key format.');
      return;
    }

    setIsAdding(true);
    try {
      await addNewAccount(newPublicKey.trim(), newSecretKey.trim(), newLabel.trim());
      setShowAddModal(false);
      setNewPublicKey('');
      setNewSecretKey('');
      setNewLabel('');
    } catch {
      Alert.alert('Error', 'Failed to add account.');
    } finally {
      setIsAdding(false);
    }
  };

  const renderAccount = ({ item }: { item: StellarAccount }) => {
    const isActive = item.publicKey === activeAccount?.publicKey;
    return (
      <TouchableOpacity
        style={[styles.accountCard, isActive && styles.activeCard]}
        onPress={() => handleSwitch(item.publicKey)}
        onLongPress={() => handleRemove(item)}
        disabled={isSwitching}
        accessibilityRole="button"
        accessibilityLabel={`${item.label}, ${truncateKey(item.publicKey)}${isActive ? ', active' : ''}`}
        accessibilityHint={isActive ? 'Currently active account' : 'Double tap to switch, long press to remove'}
      >
        <View style={styles.accountAvatar}>
          <Text style={styles.avatarText}>
            {item.label.charAt(0).toUpperCase()}
          </Text>
        </View>
        <View style={styles.accountInfo}>
          <Text style={styles.accountLabel}>{item.label}</Text>
          <Text style={styles.accountKey}>{truncateKey(item.publicKey)}</Text>
        </View>
        {isActive && (
          <View style={styles.activeBadge}>
            <Text style={styles.activeBadgeText}>Active</Text>
          </View>
        )}
        {isSwitching && !isActive && (
          <ActivityIndicator size="small" color="#3B82F6" />
        )}
      </TouchableOpacity>
    );
  };

  if (isLoading) {
    return (
      <View style={styles.centered}>
        <ActivityIndicator size="large" color="#3B82F6" />
        <Text style={styles.loadingText}>Loading accounts...</Text>
      </View>
    );
  }

  return (
    <View style={styles.container}>
      <View style={styles.header}>
        <Text style={styles.title}>Accounts</Text>
        <View style={styles.headerActions}>
          {isKeyDecrypted && (
            <TouchableOpacity
              style={styles.lockButton}
              onPress={lock}
              accessibilityLabel="Lock active key"
            >
              <Text style={styles.lockButtonText}>Lock</Text>
            </TouchableOpacity>
          )}
        </View>
      </View>

      {activeAccount && (
        <View style={styles.activeSection}>
          <Text style={styles.sectionLabel}>Active Account</Text>
          <View style={styles.activeAccountCard}>
            <View style={styles.activeAvatar}>
              <Text style={styles.activeAvatarText}>
                {activeAccount.label.charAt(0).toUpperCase()}
              </Text>
            </View>
            <View style={styles.accountInfo}>
              <Text style={styles.activeAccountLabel}>{activeAccount.label}</Text>
              <Text style={styles.activeAccountKey}>
                {truncateKey(activeAccount.publicKey)}
              </Text>
            </View>
            <View
              style={[styles.keyStatus, isKeyDecrypted ? styles.keyDecrypted : styles.keyLocked]}
            >
              <Text style={styles.keyStatusText}>
                {isKeyDecrypted ? 'Unlocked' : 'Locked'}
              </Text>
            </View>
          </View>
        </View>
      )}

      <Text style={styles.sectionLabel}>All Accounts</Text>
      <FlatList
        data={accounts}
        keyExtractor={(item) => item.publicKey}
        renderItem={renderAccount}
        contentContainerStyle={styles.list}
        ListEmptyComponent={
          <View style={styles.emptyState}>
            <Text style={styles.emptyText}>
              No accounts added yet. Tap "Add Account" to get started.
            </Text>
          </View>
        }
      />

      <TouchableOpacity
        style={styles.addButton}
        onPress={() => setShowAddModal(true)}
        accessibilityRole="button"
        accessibilityLabel="Add new Stellar account"
      >
        <Text style={styles.addButtonText}>+ Add Account</Text>
      </TouchableOpacity>

      <Modal
        visible={showAddModal}
        animationType="slide"
        transparent
        onRequestClose={() => setShowAddModal(false)}
      >
        <View style={styles.modalOverlay}>
          <View style={styles.modalContent}>
            <Text style={styles.modalTitle}>Add Account</Text>

            <TextInput
              style={styles.input}
              placeholder="Account label"
              value={newLabel}
              onChangeText={setNewLabel}
              autoFocus
              accessibilityLabel="Account label"
            />
            <TextInput
              style={styles.input}
              placeholder="Public key (G...)"
              value={newPublicKey}
              onChangeText={setNewPublicKey}
              autoCapitalize="characters"
              accessibilityLabel="Stellar public key"
            />
            <TextInput
              style={styles.input}
              placeholder="Secret key (S...)"
              value={newSecretKey}
              onChangeText={setNewSecretKey}
              autoCapitalize="characters"
              secureTextEntry
              accessibilityLabel="Stellar secret key"
            />

            <View style={styles.modalActions}>
              <TouchableOpacity
                style={styles.cancelButton}
                onPress={() => {
                  setShowAddModal(false);
                  setNewPublicKey('');
                  setNewSecretKey('');
                  setNewLabel('');
                }}
              >
                <Text style={styles.cancelButtonText}>Cancel</Text>
              </TouchableOpacity>
              <TouchableOpacity
                style={[styles.confirmButton, isAdding && styles.disabledButton]}
                onPress={handleAdd}
                disabled={isAdding}
              >
                {isAdding ? (
                  <ActivityIndicator size="small" color="#fff" />
                ) : (
                  <Text style={styles.confirmButtonText}>Add</Text>
                )}
              </TouchableOpacity>
            </View>
          </View>
        </View>
      </Modal>
    </View>
  );
}

const styles = StyleSheet.create({
  container: {
    flex: 1,
    backgroundColor: '#F9FAFB',
    padding: 16,
  },
  centered: {
    flex: 1,
    justifyContent: 'center',
    alignItems: 'center',
  },
  loadingText: {
    marginTop: 12,
    fontSize: 14,
    color: '#6B7280',
  },
  header: {
    flexDirection: 'row',
    justifyContent: 'space-between',
    alignItems: 'center',
    marginBottom: 20,
  },
  headerActions: {
    flexDirection: 'row',
    gap: 8,
  },
  title: {
    fontSize: 24,
    fontWeight: '700',
    color: '#111827',
  },
  lockButton: {
    paddingHorizontal: 12,
    paddingVertical: 6,
    backgroundColor: '#EF4444',
    borderRadius: 8,
  },
  lockButtonText: {
    color: '#fff',
    fontSize: 13,
    fontWeight: '600',
  },
  sectionLabel: {
    fontSize: 13,
    fontWeight: '600',
    color: '#6B7280',
    textTransform: 'uppercase',
    letterSpacing: 0.5,
    marginBottom: 8,
    marginTop: 16,
  },
  activeSection: {
    marginBottom: 8,
  },
  activeAccountCard: {
    flexDirection: 'row',
    alignItems: 'center',
    padding: 16,
    backgroundColor: '#EFF6FF',
    borderRadius: 12,
    borderWidth: 2,
    borderColor: '#3B82F6',
  },
  activeAvatar: {
    width: 44,
    height: 44,
    borderRadius: 22,
    backgroundColor: '#3B82F6',
    justifyContent: 'center',
    alignItems: 'center',
    marginRight: 12,
  },
  activeAvatarText: {
    color: '#fff',
    fontSize: 18,
    fontWeight: '700',
  },
  activeAccountLabel: {
    fontSize: 16,
    fontWeight: '600',
    color: '#1E40AF',
  },
  activeAccountKey: {
    fontSize: 12,
    color: '#60A5FA',
    fontFamily: 'monospace',
  },
  keyStatus: {
    marginLeft: 'auto',
    paddingHorizontal: 10,
    paddingVertical: 4,
    borderRadius: 12,
  },
  keyDecrypted: {
    backgroundColor: '#D1FAE5',
  },
  keyLocked: {
    backgroundColor: '#FEE2E2',
  },
  keyStatusText: {
    fontSize: 11,
    fontWeight: '600',
  },
  list: {
    paddingBottom: 16,
  },
  accountCard: {
    flexDirection: 'row',
    alignItems: 'center',
    padding: 14,
    backgroundColor: '#fff',
    borderRadius: 12,
    marginBottom: 8,
    borderWidth: 1,
    borderColor: '#E5E7EB',
  },
  activeCard: {
    borderColor: '#3B82F6',
    backgroundColor: '#F0F9FF',
  },
  accountAvatar: {
    width: 40,
    height: 40,
    borderRadius: 20,
    backgroundColor: '#E5E7EB',
    justifyContent: 'center',
    alignItems: 'center',
    marginRight: 12,
  },
  avatarText: {
    color: '#4B5563',
    fontSize: 16,
    fontWeight: '600',
  },
  accountInfo: {
    flex: 1,
  },
  accountLabel: {
    fontSize: 15,
    fontWeight: '600',
    color: '#111827',
  },
  accountKey: {
    fontSize: 12,
    color: '#9CA3AF',
    fontFamily: 'monospace',
  },
  activeBadge: {
    paddingHorizontal: 8,
    paddingVertical: 3,
    backgroundColor: '#DBEAFE',
    borderRadius: 8,
  },
  activeBadgeText: {
    fontSize: 11,
    fontWeight: '600',
    color: '#2563EB',
  },
  emptyState: {
    padding: 32,
    alignItems: 'center',
  },
  emptyText: {
    fontSize: 14,
    color: '#9CA3AF',
    textAlign: 'center',
  },
  addButton: {
    backgroundColor: '#3B82F6',
    paddingVertical: 14,
    borderRadius: 12,
    alignItems: 'center',
    marginTop: 8,
  },
  addButtonText: {
    color: '#fff',
    fontSize: 16,
    fontWeight: '600',
  },
  modalOverlay: {
    flex: 1,
    justifyContent: 'flex-end',
    backgroundColor: 'rgba(0,0,0,0.4)',
  },
  modalContent: {
    backgroundColor: '#fff',
    borderTopLeftRadius: 20,
    borderTopRightRadius: 20,
    padding: 24,
    paddingBottom: 40,
  },
  modalTitle: {
    fontSize: 20,
    fontWeight: '700',
    color: '#111827',
    marginBottom: 20,
  },
  input: {
    borderWidth: 1,
    borderColor: '#D1D5DB',
    borderRadius: 10,
    paddingHorizontal: 14,
    paddingVertical: 12,
    fontSize: 15,
    marginBottom: 12,
    color: '#111827',
  },
  modalActions: {
    flexDirection: 'row',
    gap: 12,
    marginTop: 8,
  },
  cancelButton: {
    flex: 1,
    paddingVertical: 14,
    borderRadius: 10,
    backgroundColor: '#F3F4F6',
    alignItems: 'center',
  },
  cancelButtonText: {
    fontSize: 15,
    fontWeight: '600',
    color: '#4B5563',
  },
  confirmButton: {
    flex: 1,
    paddingVertical: 14,
    borderRadius: 10,
    backgroundColor: '#3B82F6',
    alignItems: 'center',
  },
  disabledButton: {
    opacity: 0.6,
  },
  confirmButtonText: {
    fontSize: 15,
    fontWeight: '600',
    color: '#fff',
  },
});
