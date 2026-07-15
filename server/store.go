package main

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"time"
)

var ErrRevisionConflict = errors.New("revision 必须大于服务器中的当前值")

type StoredSnapshot struct {
	Status Status
	Body   []byte
	ETag   string
}

type FileStore struct {
	mu      sync.RWMutex
	path    string
	current *StoredSnapshot
}

func NewFileStore(path string) (*FileStore, error) {
	if path == "" {
		return nil, errors.New("DATA_FILE 不能为空")
	}
	store := &FileStore{path: path}
	if err := store.load(); err != nil {
		return nil, err
	}
	return store, nil
}

func (store *FileStore) load() error {
	body, err := os.ReadFile(store.path)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("读取状态文件: %w", err)
	}
	if len(body) > maxRequestBodyBytes {
		return fmt.Errorf("状态文件超过 %d 字节限制", maxRequestBodyBytes)
	}
	status, err := decodeAndValidateStatus(body)
	if err != nil {
		return fmt.Errorf("状态文件无效: %w", err)
	}
	canonical, err := json.Marshal(status)
	if err != nil {
		return fmt.Errorf("规范化状态文件: %w", err)
	}
	store.current = makeStoredSnapshot(status, canonical)
	return nil
}

func (store *FileStore) Get() (StoredSnapshot, bool) {
	store.mu.RLock()
	defer store.mu.RUnlock()
	if store.current == nil {
		return StoredSnapshot{}, false
	}
	return cloneStoredSnapshot(*store.current), true
}

func (store *FileStore) Put(status Status, receivedAt time.Time) (StoredSnapshot, error) {
	store.mu.Lock()
	defer store.mu.Unlock()

	if store.current != nil && status.Revision <= store.current.Status.Revision {
		return StoredSnapshot{}, fmt.Errorf("%w（当前 %d，收到 %d）", ErrRevisionConflict, store.current.Status.Revision, status.Revision)
	}

	status.ReceivedAt = receivedAt.UTC().Format(time.RFC3339Nano)
	body, err := json.Marshal(status)
	if err != nil {
		return StoredSnapshot{}, fmt.Errorf("编码状态: %w", err)
	}
	if len(body) > maxRequestBodyBytes {
		return StoredSnapshot{}, fmt.Errorf("服务器补充 receivedAt 后状态超过 %d 字节限制", maxRequestBodyBytes)
	}
	if err := writeFileAtomic(store.path, body, 0o600); err != nil {
		return StoredSnapshot{}, fmt.Errorf("持久化状态: %w", err)
	}

	stored := makeStoredSnapshot(status, body)
	store.current = stored
	return cloneStoredSnapshot(*stored), nil
}

func makeStoredSnapshot(status Status, body []byte) *StoredSnapshot {
	digest := sha256.Sum256(body)
	return &StoredSnapshot{
		Status: status,
		Body:   append([]byte(nil), body...),
		ETag:   `"` + hex.EncodeToString(digest[:]) + `"`,
	}
}

func cloneStoredSnapshot(snapshot StoredSnapshot) StoredSnapshot {
	snapshot.Body = append([]byte(nil), snapshot.Body...)
	return snapshot
}

func writeFileAtomic(path string, body []byte, mode os.FileMode) (returnErr error) {
	directory := filepath.Dir(path)
	if err := os.MkdirAll(directory, 0o700); err != nil {
		return fmt.Errorf("创建数据目录: %w", err)
	}

	temporary, err := os.CreateTemp(directory, "."+filepath.Base(path)+".tmp-*")
	if err != nil {
		return fmt.Errorf("创建临时文件: %w", err)
	}
	temporaryPath := temporary.Name()
	defer func() {
		_ = temporary.Close()
		if returnErr != nil {
			_ = os.Remove(temporaryPath)
		}
	}()

	if err := temporary.Chmod(mode); err != nil {
		return fmt.Errorf("设置临时文件权限: %w", err)
	}
	if _, err := temporary.Write(body); err != nil {
		return fmt.Errorf("写入临时文件: %w", err)
	}
	if err := temporary.Sync(); err != nil {
		return fmt.Errorf("同步临时文件: %w", err)
	}
	if err := temporary.Close(); err != nil {
		return fmt.Errorf("关闭临时文件: %w", err)
	}
	if err := replaceFileAtomic(temporaryPath, path); err != nil {
		return fmt.Errorf("原子替换状态文件: %w", err)
	}
	if err := syncParentDirectory(directory); err != nil {
		return fmt.Errorf("同步数据目录: %w", err)
	}
	return nil
}
