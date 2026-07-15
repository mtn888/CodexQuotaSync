package main

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"strconv"
	"strings"
	"time"
)

const (
	maxRequestBodyBytes = 16 * 1024
	allowedClockSkew    = 5 * time.Minute
	writeRateLimit      = 30
	writeRateWindow     = time.Minute
)

type API struct {
	store   *FileStore
	secret  []byte
	now     func() time.Time
	limiter *fixedWindowLimiter
	mux     *http.ServeMux
}

func NewAPI(store *FileStore, secret string) (*API, error) {
	if store == nil {
		return nil, errors.New("store 不能为空")
	}
	if secret == "" {
		return nil, errors.New("CQS_WRITE_SECRET 不能为空")
	}
	api := &API{
		store:   store,
		secret:  []byte(secret),
		now:     time.Now,
		limiter: newFixedWindowLimiter(writeRateLimit, writeRateWindow),
		mux:     http.NewServeMux(),
	}
	api.mux.HandleFunc("/healthz", api.handleHealth)
	api.mux.HandleFunc("/v1/status", api.handleStatus)
	return api, nil
}

func (api *API) ServeHTTP(response http.ResponseWriter, request *http.Request) {
	api.mux.ServeHTTP(response, request)
}

func (api *API) handleHealth(response http.ResponseWriter, request *http.Request) {
	if request.Method != http.MethodGet {
		methodNotAllowed(response, http.MethodGet)
		return
	}
	response.Header().Set("Cache-Control", "no-store")
	writeJSON(response, http.StatusOK, map[string]string{"status": "ok"})
}

func (api *API) handleStatus(response http.ResponseWriter, request *http.Request) {
	switch request.Method {
	case http.MethodGet:
		api.getStatus(response, request)
	case http.MethodPut:
		api.putStatus(response, request)
	default:
		methodNotAllowed(response, http.MethodGet, http.MethodPut)
	}
}

func (api *API) getStatus(response http.ResponseWriter, request *http.Request) {
	snapshot, ok := api.store.Get()
	if !ok {
		writeError(response, http.StatusNotFound, "status_not_found", "服务器还没有收到状态快照")
		return
	}
	response.Header().Set("Cache-Control", "no-cache")
	response.Header().Set("ETag", snapshot.ETag)
	if etagMatches(request.Header.Get("If-None-Match"), snapshot.ETag) {
		response.WriteHeader(http.StatusNotModified)
		return
	}
	response.Header().Set("Content-Type", "application/json; charset=utf-8")
	response.WriteHeader(http.StatusOK)
	_, _ = response.Write(snapshot.Body)
}

func (api *API) putStatus(response http.ResponseWriter, request *http.Request) {
	now := api.now()
	client := remoteClient(request.RemoteAddr)
	if allowed, retryAfter := api.limiter.Allow(client, now); !allowed {
		seconds := int(retryAfter.Round(time.Second) / time.Second)
		if seconds < 1 {
			seconds = 1
		}
		response.Header().Set("Retry-After", strconv.Itoa(seconds))
		writeError(response, http.StatusTooManyRequests, "rate_limited", "写请求过于频繁")
		return
	}

	body, err := readLimitedBody(response, request)
	if err != nil {
		var tooLarge *http.MaxBytesError
		if errors.As(err, &tooLarge) {
			writeError(response, http.StatusRequestEntityTooLarge, "body_too_large", "请求体不能超过 16384 字节")
			return
		}
		writeError(response, http.StatusBadRequest, "read_failed", "无法读取请求体")
		return
	}

	timestamp, signature, err := authenticationHeaders(request)
	if err != nil {
		writeError(response, http.StatusUnauthorized, "invalid_auth", err.Error())
		return
	}
	seconds, err := parseUnixTimestamp(timestamp)
	if err != nil || !timestampInWindow(seconds, now, allowedClockSkew) {
		writeError(response, http.StatusUnauthorized, "invalid_timestamp", "X-CQS-Timestamp 必须是服务器时间前后 5 分钟内的 Unix 秒")
		return
	}
	if !validSignature(api.secret, timestamp, body, signature) {
		writeError(response, http.StatusUnauthorized, "invalid_signature", "HMAC 签名无效")
		return
	}

	status, err := decodeAndValidateStatus(body)
	if err != nil {
		writeError(response, http.StatusBadRequest, "invalid_status", err.Error())
		return
	}
	stored, err := api.store.Put(status, now)
	if errors.Is(err, ErrRevisionConflict) {
		writeError(response, http.StatusConflict, "revision_conflict", err.Error())
		return
	}
	if err != nil {
		writeError(response, http.StatusInternalServerError, "storage_failed", "无法保存状态")
		return
	}

	response.Header().Set("Cache-Control", "no-store")
	response.Header().Set("ETag", stored.ETag)
	response.Header().Set("Content-Type", "application/json; charset=utf-8")
	response.WriteHeader(http.StatusOK)
	_, _ = response.Write(stored.Body)
}

func readLimitedBody(response http.ResponseWriter, request *http.Request) ([]byte, error) {
	defer request.Body.Close()
	request.Body = http.MaxBytesReader(response, request.Body, maxRequestBodyBytes)
	return io.ReadAll(request.Body)
}

func authenticationHeaders(request *http.Request) (string, string, error) {
	timestamps := request.Header.Values("X-CQS-Timestamp")
	signatures := request.Header.Values("X-CQS-Signature")
	if len(timestamps) != 1 || len(signatures) != 1 || timestamps[0] == "" || signatures[0] == "" {
		return "", "", errors.New("需要且只能提供一个 X-CQS-Timestamp 和 X-CQS-Signature")
	}
	return timestamps[0], signatures[0], nil
}

func parseUnixTimestamp(value string) (int64, error) {
	seconds, err := strconv.ParseInt(value, 10, 64)
	if err != nil || strconv.FormatInt(seconds, 10) != value {
		return 0, errors.New("时间戳格式无效")
	}
	return seconds, nil
}

func timestampInWindow(seconds int64, now time.Time, window time.Duration) bool {
	difference := now.Sub(time.Unix(seconds, 0))
	return difference >= -window && difference <= window
}

func validSignature(secret []byte, timestamp string, body []byte, header string) bool {
	if !strings.HasPrefix(header, "v1=") || len(header) != len("v1=")+sha256.Size*2 {
		return false
	}
	provided, err := hex.DecodeString(strings.TrimPrefix(header, "v1="))
	if err != nil || len(provided) != sha256.Size {
		return false
	}
	bodyHash := sha256.Sum256(body)
	canonical := fmt.Sprintf("PUT\n/v1/status\n%s\n%s", timestamp, hex.EncodeToString(bodyHash[:]))
	mac := hmac.New(sha256.New, secret)
	_, _ = mac.Write([]byte(canonical))
	return hmac.Equal(provided, mac.Sum(nil))
}

func remoteClient(remoteAddress string) string {
	host, _, err := net.SplitHostPort(remoteAddress)
	if err == nil && host != "" {
		return host
	}
	if remoteAddress == "" {
		return "unknown"
	}
	return remoteAddress
}

func etagMatches(ifNoneMatch, etag string) bool {
	for _, candidate := range strings.Split(ifNoneMatch, ",") {
		candidate = strings.TrimSpace(candidate)
		if candidate == "*" || candidate == etag || strings.TrimPrefix(candidate, "W/") == etag {
			return true
		}
	}
	return false
}

func methodNotAllowed(response http.ResponseWriter, methods ...string) {
	response.Header().Set("Allow", strings.Join(methods, ", "))
	writeError(response, http.StatusMethodNotAllowed, "method_not_allowed", "请求方法不受支持")
}

func writeError(response http.ResponseWriter, status int, code, message string) {
	response.Header().Set("Cache-Control", "no-store")
	writeJSON(response, status, map[string]any{
		"error": map[string]string{
			"code":    code,
			"message": message,
		},
	})
}

func writeJSON(response http.ResponseWriter, status int, value any) {
	response.Header().Set("Content-Type", "application/json; charset=utf-8")
	response.WriteHeader(status)
	_ = json.NewEncoder(response).Encode(value)
}
