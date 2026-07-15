package main

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

const testSecret = "test-secret-with-enough-randomness"

func TestHealthAndEmptyStatus(t *testing.T) {
	api := newTestAPI(t)

	health := httptest.NewRecorder()
	api.ServeHTTP(health, httptest.NewRequest(http.MethodGet, "/healthz", nil))
	if health.Code != http.StatusOK || !strings.Contains(health.Body.String(), `"status":"ok"`) {
		t.Fatalf("health response = %d %s", health.Code, health.Body.String())
	}

	status := httptest.NewRecorder()
	api.ServeHTTP(status, httptest.NewRequest(http.MethodGet, "/v1/status", nil))
	if status.Code != http.StatusNotFound {
		t.Fatalf("status response = %d %s", status.Code, status.Body.String())
	}
}

func TestPutGetAndConditionalGet(t *testing.T) {
	api := newTestAPI(t)
	now := time.Date(2026, 7, 15, 10, 0, 0, 0, time.UTC)
	api.now = func() time.Time { return now }
	incoming := validStatus(42)
	incoming.ReceivedAt = "2000-01-01T00:00:00Z"
	body := mustStatusJSON(t, incoming)

	put := performSignedPut(t, api, body, now.Unix())
	if put.Code != http.StatusOK {
		t.Fatalf("PUT response = %d %s", put.Code, put.Body.String())
	}
	if !strings.Contains(put.Body.String(), `"receivedAt":"2026-07-15T10:00:00Z"`) {
		t.Fatalf("server did not overwrite receivedAt: %s", put.Body.String())
	}
	etag := put.Header().Get("ETag")
	if etag == "" {
		t.Fatal("PUT missing ETag")
	}

	get := httptest.NewRecorder()
	api.ServeHTTP(get, httptest.NewRequest(http.MethodGet, "/v1/status", nil))
	if get.Code != http.StatusOK || get.Header().Get("ETag") != etag || get.Body.String() != put.Body.String() {
		t.Fatalf("GET response = %d, etag %q, body %s", get.Code, get.Header().Get("ETag"), get.Body.String())
	}

	conditionalRequest := httptest.NewRequest(http.MethodGet, "/v1/status", nil)
	conditionalRequest.Header.Set("If-None-Match", etag)
	conditional := httptest.NewRecorder()
	api.ServeHTTP(conditional, conditionalRequest)
	if conditional.Code != http.StatusNotModified || conditional.Body.Len() != 0 {
		t.Fatalf("conditional GET = %d %s", conditional.Code, conditional.Body.String())
	}
}

func TestPutAuthenticationAndValidation(t *testing.T) {
	now := time.Date(2026, 7, 15, 10, 0, 0, 0, time.UTC)
	tests := []struct {
		name   string
		make   func(t *testing.T, api *API) *httptest.ResponseRecorder
		status int
		code   string
	}{
		{
			name: "missing auth",
			make: func(t *testing.T, api *API) *httptest.ResponseRecorder {
				request := httptest.NewRequest(http.MethodPut, "/v1/status", strings.NewReader(`{}`))
				response := httptest.NewRecorder()
				api.ServeHTTP(response, request)
				return response
			},
			status: http.StatusUnauthorized, code: "invalid_auth",
		},
		{
			name: "expired timestamp",
			make: func(t *testing.T, api *API) *httptest.ResponseRecorder {
				return performSignedPut(t, api, mustStatusJSON(t, validStatus(1)), now.Add(-6*time.Minute).Unix())
			},
			status: http.StatusUnauthorized, code: "invalid_timestamp",
		},
		{
			name: "bad signature",
			make: func(t *testing.T, api *API) *httptest.ResponseRecorder {
				body := mustStatusJSON(t, validStatus(1))
				request := httptest.NewRequest(http.MethodPut, "/v1/status", strings.NewReader(string(body)))
				request.Header.Set("X-CQS-Timestamp", fmt.Sprint(now.Unix()))
				request.Header.Set("X-CQS-Signature", "v1="+strings.Repeat("0", 64))
				response := httptest.NewRecorder()
				api.ServeHTTP(response, request)
				return response
			},
			status: http.StatusUnauthorized, code: "invalid_signature",
		},
		{
			name: "invalid status",
			make: func(t *testing.T, api *API) *httptest.ResponseRecorder {
				return performSignedPut(t, api, []byte(`{}`), now.Unix())
			},
			status: http.StatusBadRequest, code: "invalid_status",
		},
		{
			name: "too large",
			make: func(t *testing.T, api *API) *httptest.ResponseRecorder {
				request := httptest.NewRequest(http.MethodPut, "/v1/status", strings.NewReader(strings.Repeat("x", maxRequestBodyBytes+1)))
				response := httptest.NewRecorder()
				api.ServeHTTP(response, request)
				return response
			},
			status: http.StatusRequestEntityTooLarge, code: "body_too_large",
		},
	}

	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			api := newTestAPI(t)
			api.now = func() time.Time { return now }
			response := test.make(t, api)
			if response.Code != test.status || !strings.Contains(response.Body.String(), test.code) {
				t.Fatalf("response = %d %s, want %d and %q", response.Code, response.Body.String(), test.status, test.code)
			}
		})
	}
}

func TestPutRejectsRevisionConflict(t *testing.T) {
	api := newTestAPI(t)
	now := time.Date(2026, 7, 15, 10, 0, 0, 0, time.UTC)
	api.now = func() time.Time { return now }
	if response := performSignedPut(t, api, mustStatusJSON(t, validStatus(4)), now.Unix()); response.Code != http.StatusOK {
		t.Fatal(response.Body.String())
	}
	response := performSignedPut(t, api, mustStatusJSON(t, validStatus(4)), now.Unix())
	if response.Code != http.StatusConflict || !strings.Contains(response.Body.String(), "revision_conflict") {
		t.Fatalf("response = %d %s", response.Code, response.Body.String())
	}
}

func TestWriteRateLimit(t *testing.T) {
	api := newTestAPI(t)
	now := time.Date(2026, 7, 15, 10, 0, 0, 0, time.UTC)
	api.now = func() time.Time { return now }
	api.limiter = newFixedWindowLimiter(1, time.Minute)

	first := httptest.NewRequest(http.MethodPut, "/v1/status", strings.NewReader(`{}`))
	first.RemoteAddr = "192.0.2.1:1000"
	firstResponse := httptest.NewRecorder()
	api.ServeHTTP(firstResponse, first)
	if firstResponse.Code != http.StatusUnauthorized {
		t.Fatalf("first response = %d", firstResponse.Code)
	}

	second := httptest.NewRequest(http.MethodPut, "/v1/status", strings.NewReader(`{}`))
	second.RemoteAddr = "192.0.2.1:1001"
	secondResponse := httptest.NewRecorder()
	api.ServeHTTP(secondResponse, second)
	if secondResponse.Code != http.StatusTooManyRequests || secondResponse.Header().Get("Retry-After") == "" {
		t.Fatalf("second response = %d, retry-after %q", secondResponse.Code, secondResponse.Header().Get("Retry-After"))
	}
}

func TestMethodNotAllowed(t *testing.T) {
	api := newTestAPI(t)
	response := httptest.NewRecorder()
	api.ServeHTTP(response, httptest.NewRequest(http.MethodPost, "/v1/status", nil))
	if response.Code != http.StatusMethodNotAllowed || response.Header().Get("Allow") != "GET, PUT" {
		t.Fatalf("response = %d, Allow = %q", response.Code, response.Header().Get("Allow"))
	}
}

func newTestAPI(t *testing.T) *API {
	t.Helper()
	store, err := NewFileStore(filepath.Join(t.TempDir(), "state.json"))
	if err != nil {
		t.Fatal(err)
	}
	api, err := NewAPI(store, testSecret)
	if err != nil {
		t.Fatal(err)
	}
	return api
}

func performSignedPut(t *testing.T, api *API, body []byte, unixSeconds int64) *httptest.ResponseRecorder {
	t.Helper()
	timestamp := fmt.Sprint(unixSeconds)
	request := httptest.NewRequest(http.MethodPut, "/v1/status", strings.NewReader(string(body)))
	request.Header.Set("X-CQS-Timestamp", timestamp)
	request.Header.Set("X-CQS-Signature", signBody(testSecret, timestamp, body))
	response := httptest.NewRecorder()
	api.ServeHTTP(response, request)
	return response
}

func signBody(secret, timestamp string, body []byte) string {
	bodyHash := sha256.Sum256(body)
	canonical := fmt.Sprintf("PUT\n/v1/status\n%s\n%s", timestamp, hex.EncodeToString(bodyHash[:]))
	mac := hmac.New(sha256.New, []byte(secret))
	_, _ = io.WriteString(mac, canonical)
	return "v1=" + hex.EncodeToString(mac.Sum(nil))
}
