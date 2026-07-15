package main

import (
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestFileStorePersistsAndReloads(t *testing.T) {
	path := filepath.Join(t.TempDir(), "nested", "state.json")
	store, err := NewFileStore(path)
	if err != nil {
		t.Fatal(err)
	}
	receivedAt := time.Date(2026, 7, 15, 10, 1, 2, 300, time.UTC)
	stored, err := store.Put(validStatus(3), receivedAt)
	if err != nil {
		t.Fatal(err)
	}
	if stored.Status.ReceivedAt != "2026-07-15T10:01:02.0000003Z" {
		t.Fatalf("receivedAt = %q", stored.Status.ReceivedAt)
	}
	if _, err := os.Stat(path); err != nil {
		t.Fatalf("state file missing: %v", err)
	}

	reloaded, err := NewFileStore(path)
	if err != nil {
		t.Fatal(err)
	}
	got, ok := reloaded.Get()
	if !ok || got.Status.Revision != 3 || got.ETag != stored.ETag {
		t.Fatalf("reloaded = %+v, ok = %v", got, ok)
	}
	got.Body[0] = 'x'
	again, _ := reloaded.Get()
	if again.Body[0] == 'x' {
		t.Fatal("Get returned mutable store memory")
	}
}

func TestFileStoreRejectsNonIncreasingRevision(t *testing.T) {
	store, err := NewFileStore(filepath.Join(t.TempDir(), "state.json"))
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now()
	if _, err := store.Put(validStatus(5), now); err != nil {
		t.Fatal(err)
	}
	for _, revision := range []int64{5, 4} {
		if _, err := store.Put(validStatus(revision), now); !errors.Is(err, ErrRevisionConflict) {
			t.Fatalf("revision %d error = %v", revision, err)
		}
	}
	if _, err := store.Put(validStatus(6), now.Add(time.Second)); err != nil {
		t.Fatalf("atomic replacement failed: %v", err)
	}
	reloaded, err := NewFileStore(store.path)
	if err != nil {
		t.Fatal(err)
	}
	latest, ok := reloaded.Get()
	if !ok || latest.Status.Revision != 6 {
		t.Fatalf("latest revision = %d, ok = %v", latest.Status.Revision, ok)
	}
}

func TestNewFileStoreRejectsCorruptState(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.json")
	if err := os.WriteFile(path, []byte(`{"schemaVersion":1}`), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := NewFileStore(path); err == nil {
		t.Fatal("expected corrupt state error")
	}
}
