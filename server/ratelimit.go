package main

import (
	"sync"
	"time"
)

type rateWindow struct {
	startedAt time.Time
	count     int
}

type fixedWindowLimiter struct {
	mu      sync.Mutex
	limit   int
	window  time.Duration
	clients map[string]rateWindow
}

func newFixedWindowLimiter(limit int, window time.Duration) *fixedWindowLimiter {
	return &fixedWindowLimiter{
		limit:   limit,
		window:  window,
		clients: make(map[string]rateWindow),
	}
}

func (limiter *fixedWindowLimiter) Allow(client string, now time.Time) (bool, time.Duration) {
	limiter.mu.Lock()
	defer limiter.mu.Unlock()

	current := limiter.clients[client]
	if current.startedAt.IsZero() || now.Sub(current.startedAt) >= limiter.window || now.Before(current.startedAt) {
		current = rateWindow{startedAt: now}
	}
	if current.count >= limiter.limit {
		return false, limiter.window - now.Sub(current.startedAt)
	}
	current.count++
	limiter.clients[client] = current

	if len(limiter.clients) > 1024 {
		for key, entry := range limiter.clients {
			if now.Sub(entry.startedAt) >= limiter.window {
				delete(limiter.clients, key)
			}
		}
	}
	return true, 0
}
