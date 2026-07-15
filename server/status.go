package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"strings"
	"time"
	"unicode/utf8"
)

const schemaVersion = 1

type Status struct {
	SchemaVersion    int               `json:"schemaVersion"`
	SourceID         string            `json:"sourceId"`
	Revision         int64             `json:"revision"`
	CollectorVersion string            `json:"collectorVersion"`
	CollectedAt      string            `json:"collectedAt"`
	ReceivedAt       string            `json:"receivedAt,omitempty"`
	Activity         Activity          `json:"activity"`
	LatestAttempt    Attempt           `json:"latestAttempt"`
	LastGoodSnapshot *ProviderSnapshot `json:"lastGoodSnapshot"`
}

type Activity struct {
	Executing          int64  `json:"executing"`
	WaitingOnApproval  int64  `json:"waitingOnApproval"`
	WaitingOnUserInput int64  `json:"waitingOnUserInput"`
	Source             string `json:"source"`
	ObservedAt         string `json:"observedAt"`
	Stale              bool   `json:"stale"`
}

type Attempt struct {
	Status      string  `json:"status"`
	Message     *string `json:"message,omitempty"`
	AttemptedAt string  `json:"attemptedAt"`
}

type UsageWindow struct {
	RemainingPercent float64 `json:"remainingPercent"`
	ResetsAt         *string `json:"resetsAt"`
	WindowSeconds    *int64  `json:"windowSeconds"`
}

type ProviderSnapshot struct {
	Provider             string       `json:"provider"`
	DisplayName          string       `json:"displayName"`
	Plan                 *string      `json:"plan"`
	ShortWindow          *UsageWindow `json:"shortWindow"`
	WeeklyWindow         *UsageWindow `json:"weeklyWindow"`
	ResetCredits         *int64       `json:"resetCredits"`
	ResetCreditExpiresAt []string     `json:"resetCreditExpiresAt"`
	UpdatedAt            string       `json:"updatedAt"`
	Status               string       `json:"status"`
	Message              *string      `json:"message"`
	NextResetAt          *string      `json:"nextResetAt"`
	NextResetWindow      *string      `json:"nextResetWindow"`
}

func decodeAndValidateStatus(body []byte) (Status, error) {
	if len(bytes.TrimSpace(body)) == 0 {
		return Status{}, errors.New("请求体不能为空")
	}

	var shape any
	shapeDecoder := json.NewDecoder(bytes.NewReader(body))
	shapeDecoder.UseNumber()
	if err := shapeDecoder.Decode(&shape); err != nil {
		return Status{}, fmt.Errorf("JSON 无效: %w", err)
	}
	if err := ensureJSONEOF(shapeDecoder); err != nil {
		return Status{}, err
	}
	if err := validateRequiredShape(shape); err != nil {
		return Status{}, err
	}

	var status Status
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&status); err != nil {
		return Status{}, fmt.Errorf("JSON 字段无效: %w", err)
	}
	if err := ensureJSONEOF(decoder); err != nil {
		return Status{}, err
	}
	if err := status.Validate(); err != nil {
		return Status{}, err
	}
	return status, nil
}

func ensureJSONEOF(decoder *json.Decoder) error {
	var trailing any
	if err := decoder.Decode(&trailing); err != io.EOF {
		if err == nil {
			return errors.New("请求体只能包含一个 JSON 对象")
		}
		return fmt.Errorf("JSON 尾部无效: %w", err)
	}
	return nil
}

func validateRequiredShape(value any) error {
	root, err := requireObject(value, "根对象")
	if err != nil {
		return err
	}
	if err := requireFields(root, "根对象", []string{
		"schemaVersion", "sourceId", "revision", "collectorVersion", "collectedAt",
		"activity", "latestAttempt", "lastGoodSnapshot",
	}, map[string]bool{"lastGoodSnapshot": true}); err != nil {
		return err
	}
	if receivedAt, exists := root["receivedAt"]; exists {
		value, ok := receivedAt.(string)
		if !ok {
			return errors.New("receivedAt 必须是 RFC 3339 日期时间")
		}
		if err := validateDateTime("receivedAt", value); err != nil {
			return err
		}
	}

	activity, err := requireObject(root["activity"], "activity")
	if err != nil {
		return err
	}
	if err := requireFields(activity, "activity", []string{
		"executing", "waitingOnApproval", "waitingOnUserInput", "source", "observedAt", "stale",
	}, nil); err != nil {
		return err
	}

	attempt, err := requireObject(root["latestAttempt"], "latestAttempt")
	if err != nil {
		return err
	}
	if err := requireFields(attempt, "latestAttempt", []string{"status", "attemptedAt"}, nil); err != nil {
		return err
	}

	if root["lastGoodSnapshot"] == nil {
		return nil
	}
	snapshot, err := requireObject(root["lastGoodSnapshot"], "lastGoodSnapshot")
	if err != nil {
		return err
	}
	if err := requireFields(snapshot, "lastGoodSnapshot", []string{
		"provider", "displayName", "plan", "shortWindow", "weeklyWindow", "resetCredits",
		"resetCreditExpiresAt", "updatedAt", "status", "message", "nextResetAt", "nextResetWindow",
	}, map[string]bool{
		"plan": true, "shortWindow": true, "weeklyWindow": true, "resetCredits": true,
		"message": true, "nextResetAt": true, "nextResetWindow": true,
	}); err != nil {
		return err
	}

	for _, name := range []string{"shortWindow", "weeklyWindow"} {
		if snapshot[name] == nil {
			continue
		}
		window, err := requireObject(snapshot[name], "lastGoodSnapshot."+name)
		if err != nil {
			return err
		}
		if err := requireFields(window, "lastGoodSnapshot."+name,
			[]string{"remainingPercent", "resetsAt", "windowSeconds"},
			map[string]bool{"resetsAt": true, "windowSeconds": true}); err != nil {
			return err
		}
	}
	return nil
}

func requireObject(value any, path string) (map[string]any, error) {
	object, ok := value.(map[string]any)
	if !ok {
		return nil, fmt.Errorf("%s 必须是对象", path)
	}
	return object, nil
}

func requireFields(object map[string]any, path string, names []string, nullable map[string]bool) error {
	for _, name := range names {
		value, ok := object[name]
		if !ok {
			return fmt.Errorf("%s 缺少必填字段 %s", path, name)
		}
		if value == nil && !nullable[name] {
			return fmt.Errorf("%s.%s 不能为 null", path, name)
		}
	}
	return nil
}

func (status Status) Validate() error {
	if status.SchemaVersion != schemaVersion {
		return fmt.Errorf("schemaVersion 必须为 %d", schemaVersion)
	}
	if err := validateString("sourceId", status.SourceID, 1, 64); err != nil {
		return err
	}
	if status.Revision < 0 {
		return errors.New("revision 不能小于 0")
	}
	if err := validateString("collectorVersion", status.CollectorVersion, 1, 32); err != nil {
		return err
	}
	if err := validateDateTime("collectedAt", status.CollectedAt); err != nil {
		return err
	}
	if status.ReceivedAt != "" {
		if err := validateDateTime("receivedAt", status.ReceivedAt); err != nil {
			return err
		}
	}
	if err := status.Activity.Validate(); err != nil {
		return err
	}
	if err := status.LatestAttempt.Validate(); err != nil {
		return err
	}
	if status.LastGoodSnapshot != nil {
		if err := status.LastGoodSnapshot.Validate(); err != nil {
			return err
		}
	}
	return nil
}

func (activity Activity) Validate() error {
	if activity.Executing < 0 || activity.WaitingOnApproval < 0 || activity.WaitingOnUserInput < 0 {
		return errors.New("activity 中的任务数不能小于 0")
	}
	if activity.Source != "hooks" && activity.Source != "unavailable" {
		return errors.New("activity.source 必须为 hooks 或 unavailable")
	}
	return validateDateTime("activity.observedAt", activity.ObservedAt)
}

func (attempt Attempt) Validate() error {
	if !validStatusValue(attempt.Status) {
		return errors.New("latestAttempt.status 无效")
	}
	if attempt.Message != nil && utf8.RuneCountInString(*attempt.Message) > 512 {
		return errors.New("latestAttempt.message 不能超过 512 个字符")
	}
	return validateDateTime("latestAttempt.attemptedAt", attempt.AttemptedAt)
}

func (snapshot ProviderSnapshot) Validate() error {
	if snapshot.Provider != "codex" {
		return errors.New("lastGoodSnapshot.provider 必须为 codex")
	}
	if err := validateString("lastGoodSnapshot.displayName", snapshot.DisplayName, 1, 64); err != nil {
		return err
	}
	if snapshot.Plan != nil && utf8.RuneCountInString(*snapshot.Plan) > 64 {
		return errors.New("lastGoodSnapshot.plan 不能超过 64 个字符")
	}
	if snapshot.ShortWindow != nil {
		if err := snapshot.ShortWindow.Validate("lastGoodSnapshot.shortWindow"); err != nil {
			return err
		}
	}
	if snapshot.WeeklyWindow != nil {
		if err := snapshot.WeeklyWindow.Validate("lastGoodSnapshot.weeklyWindow"); err != nil {
			return err
		}
	}
	if snapshot.ResetCredits != nil && *snapshot.ResetCredits < 0 {
		return errors.New("lastGoodSnapshot.resetCredits 不能小于 0")
	}
	if len(snapshot.ResetCreditExpiresAt) > 64 {
		return errors.New("lastGoodSnapshot.resetCreditExpiresAt 最多包含 64 项")
	}
	for index, value := range snapshot.ResetCreditExpiresAt {
		if err := validateDateTime(fmt.Sprintf("lastGoodSnapshot.resetCreditExpiresAt[%d]", index), value); err != nil {
			return err
		}
	}
	if err := validateDateTime("lastGoodSnapshot.updatedAt", snapshot.UpdatedAt); err != nil {
		return err
	}
	if !validStatusValue(snapshot.Status) {
		return errors.New("lastGoodSnapshot.status 无效")
	}
	if snapshot.Message != nil && utf8.RuneCountInString(*snapshot.Message) > 512 {
		return errors.New("lastGoodSnapshot.message 不能超过 512 个字符")
	}
	if snapshot.NextResetAt != nil {
		if err := validateDateTime("lastGoodSnapshot.nextResetAt", *snapshot.NextResetAt); err != nil {
			return err
		}
	}
	if snapshot.NextResetWindow != nil && *snapshot.NextResetWindow != "5h" && *snapshot.NextResetWindow != "weekly" {
		return errors.New("lastGoodSnapshot.nextResetWindow 必须为 5h、weekly 或 null")
	}
	return nil
}

func (window UsageWindow) Validate(path string) error {
	if math.IsNaN(window.RemainingPercent) || math.IsInf(window.RemainingPercent, 0) ||
		window.RemainingPercent < 0 || window.RemainingPercent > 100 {
		return fmt.Errorf("%s.remainingPercent 必须在 0 到 100 之间", path)
	}
	if window.ResetsAt != nil {
		if err := validateDateTime(path+".resetsAt", *window.ResetsAt); err != nil {
			return err
		}
	}
	if window.WindowSeconds != nil && *window.WindowSeconds < 0 {
		return fmt.Errorf("%s.windowSeconds 不能小于 0", path)
	}
	return nil
}

func validateString(path, value string, minimum, maximum int) error {
	length := utf8.RuneCountInString(value)
	if length < minimum || length > maximum {
		return fmt.Errorf("%s 长度必须在 %d 到 %d 个字符之间", path, minimum, maximum)
	}
	return nil
}

func validateDateTime(path, value string) error {
	if strings.TrimSpace(value) != value || value == "" {
		return fmt.Errorf("%s 必须是 RFC 3339 日期时间", path)
	}
	if _, err := time.Parse(time.RFC3339Nano, value); err != nil {
		return fmt.Errorf("%s 必须是 RFC 3339 日期时间: %w", path, err)
	}
	return nil
}

func validStatusValue(value string) bool {
	switch value {
	case "ok", "stale", "signed_out", "unavailable":
		return true
	default:
		return false
	}
}
