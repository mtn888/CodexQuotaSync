package main

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"
)

func TestDecodeAndValidateStatus(t *testing.T) {
	body := mustStatusJSON(t, validStatus(7))
	status, err := decodeAndValidateStatus(body)
	if err != nil {
		t.Fatalf("decodeAndValidateStatus() error = %v", err)
	}
	if status.Revision != 7 || status.Activity.WaitingOnApproval != 1 {
		t.Fatalf("unexpected status: %+v", status)
	}
}

func TestDecodeAndValidateStatusAllowsNullLastGoodSnapshot(t *testing.T) {
	status := validStatus(8)
	status.LastGoodSnapshot = nil
	decoded, err := decodeAndValidateStatus(mustStatusJSON(t, status))
	if err != nil {
		t.Fatalf("decodeAndValidateStatus() error = %v", err)
	}
	if decoded.LastGoodSnapshot != nil {
		t.Fatal("lastGoodSnapshot should remain null")
	}
}

func TestDecodeAndValidateStatusRejectsInvalidDocuments(t *testing.T) {
	valid := mustStatusJSON(t, validStatus(1))
	tests := []struct {
		name string
		body []byte
		want string
	}{
		{name: "unknown field", body: replaceOnce(valid, `"sourceId":"primary"`, `"sourceId":"primary","token":"secret"`), want: "unknown field"},
		{name: "missing required zero field", body: replaceOnce(valid, `"executing":2,`, ""), want: "缺少必填字段 executing"},
		{name: "null required field", body: replaceOnce(valid, `"sourceId":"primary"`, `"sourceId":null`), want: "sourceId 不能为 null"},
		{name: "null receivedAt", body: replaceOnce(valid, `"collectedAt":"2026-07-15T10:00:00Z"`, `"collectedAt":"2026-07-15T10:00:00Z","receivedAt":null`), want: "receivedAt 必须是 RFC 3339"},
		{name: "invalid date", body: replaceOnce(valid, `"collectedAt":"2026-07-15T10:00:00Z"`, `"collectedAt":"today"`), want: "collectedAt 必须是 RFC 3339"},
		{name: "invalid percentage", body: replaceOnce(valid, `"remainingPercent":74.5`, `"remainingPercent":101`), want: "remainingPercent 必须在 0 到 100"},
		{name: "trailing object", body: append(append([]byte(nil), valid...), []byte(` {}`)...), want: "只能包含一个 JSON 对象"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			_, err := decodeAndValidateStatus(test.body)
			if err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("error = %v, want containing %q", err, test.want)
			}
		})
	}
}

func validStatus(revision int64) Status {
	plan := "PRO"
	attemptMessage := "采集成功"
	providerMessage := "正常"
	shortReset := "2026-07-15T12:00:00Z"
	weeklyReset := "2026-07-19T00:00:00Z"
	shortSeconds := int64(18_000)
	weeklySeconds := int64(604_800)
	credits := int64(1)
	nextReset := shortReset
	nextWindow := "5h"
	return Status{
		SchemaVersion:    1,
		SourceID:         "primary",
		Revision:         revision,
		CollectorVersion: "0.1.0",
		CollectedAt:      "2026-07-15T10:00:00Z",
		Activity: Activity{
			Executing:          2,
			WaitingOnApproval:  1,
			WaitingOnUserInput: 1,
			Source:             "hooks",
			ObservedAt:         "2026-07-15T10:00:00Z",
			Stale:              false,
		},
		LatestAttempt: Attempt{
			Status:      "ok",
			Message:     &attemptMessage,
			AttemptedAt: "2026-07-15T10:00:00Z",
		},
		LastGoodSnapshot: &ProviderSnapshot{
			Provider:    "codex",
			DisplayName: "CODEX",
			Plan:        &plan,
			ShortWindow: &UsageWindow{
				RemainingPercent: 74.5,
				ResetsAt:         &shortReset,
				WindowSeconds:    &shortSeconds,
			},
			WeeklyWindow: &UsageWindow{
				RemainingPercent: 42,
				ResetsAt:         &weeklyReset,
				WindowSeconds:    &weeklySeconds,
			},
			ResetCredits:         &credits,
			ResetCreditExpiresAt: []string{"2026-07-16T00:00:00Z"},
			UpdatedAt:            "2026-07-15T10:00:00Z",
			Status:               "ok",
			Message:              &providerMessage,
			NextResetAt:          &nextReset,
			NextResetWindow:      &nextWindow,
		},
	}
}

func mustStatusJSON(t *testing.T, status Status) []byte {
	t.Helper()
	body, err := json.Marshal(status)
	if err != nil {
		t.Fatal(err)
	}
	return body
}

func replaceOnce(body []byte, old, replacement string) []byte {
	return bytes.Replace(body, []byte(old), []byte(replacement), 1)
}
