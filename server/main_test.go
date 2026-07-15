package main

import (
	"strings"
	"testing"
)

func TestLoadConfig(t *testing.T) {
	t.Setenv("PORT", "9080")
	t.Setenv("DATA_FILE", "test-state.json")
	t.Setenv("CQS_WRITE_SECRET", "random-secret")
	t.Setenv("TZ", "Asia/Singapore")

	config, err := loadConfig()
	if err != nil {
		t.Fatal(err)
	}
	if config.Port != 9080 || config.DataFile != "test-state.json" || config.TimeZone != "Asia/Singapore" {
		t.Fatalf("config = %+v", config)
	}
}

func TestLoadConfigRequiresSecretAndValidPort(t *testing.T) {
	t.Setenv("PORT", "8787")
	t.Setenv("CQS_WRITE_SECRET", "")
	if _, err := loadConfig(); err == nil || !strings.Contains(err.Error(), "CQS_WRITE_SECRET") {
		t.Fatalf("missing secret error = %v", err)
	}

	t.Setenv("CQS_WRITE_SECRET", "secret")
	t.Setenv("PORT", "70000")
	if _, err := loadConfig(); err == nil || !strings.Contains(err.Error(), "PORT") {
		t.Fatalf("invalid port error = %v", err)
	}
}
