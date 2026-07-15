package main

import (
	"context"
	"errors"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/signal"
	"strconv"
	"syscall"
	"time"
	_ "time/tzdata"
)

type Config struct {
	Port     int
	DataFile string
	Secret   string
	TimeZone string
}

func main() {
	if err := run(); err != nil {
		log.Fatal(err)
	}
}

func run() error {
	config, err := loadConfig()
	if err != nil {
		return err
	}
	if config.TimeZone != "" {
		location, err := time.LoadLocation(config.TimeZone)
		if err != nil {
			return fmt.Errorf("TZ 无效: %w", err)
		}
		time.Local = location
	}

	store, err := NewFileStore(config.DataFile)
	if err != nil {
		return err
	}
	api, err := NewAPI(store, config.Secret)
	if err != nil {
		return err
	}

	server := &http.Server{
		Addr:              fmt.Sprintf(":%d", config.Port),
		Handler:           api,
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       10 * time.Second,
		WriteTimeout:      10 * time.Second,
		IdleTimeout:       60 * time.Second,
		MaxHeaderBytes:    16 * 1024,
	}

	serverError := make(chan error, 1)
	go func() {
		log.Printf("Codex Quota Sync Server 正在监听 %s，数据文件 %s", server.Addr, config.DataFile)
		serverError <- server.ListenAndServe()
	}()

	stop := make(chan os.Signal, 1)
	signal.Notify(stop, os.Interrupt, syscall.SIGTERM)
	defer signal.Stop(stop)

	select {
	case err := <-serverError:
		if errors.Is(err, http.ErrServerClosed) {
			return nil
		}
		return fmt.Errorf("HTTP 服务退出: %w", err)
	case signalValue := <-stop:
		log.Printf("收到 %s，开始优雅关停", signalValue)
	}

	shutdownContext, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := server.Shutdown(shutdownContext); err != nil {
		_ = server.Close()
		return fmt.Errorf("优雅关停失败: %w", err)
	}
	if err := <-serverError; err != nil && !errors.Is(err, http.ErrServerClosed) {
		return fmt.Errorf("HTTP 服务关停: %w", err)
	}
	return nil
}

func loadConfig() (Config, error) {
	portText := envOrDefault("PORT", "8787")
	port, err := strconv.Atoi(portText)
	if err != nil || port < 1 || port > 65535 {
		return Config{}, errors.New("PORT 必须是 1 到 65535 之间的整数")
	}
	secret := os.Getenv("CQS_WRITE_SECRET")
	if secret == "" {
		return Config{}, errors.New("必须设置 CQS_WRITE_SECRET")
	}
	return Config{
		Port:     port,
		DataFile: envOrDefault("DATA_FILE", "/data/state.json"),
		Secret:   secret,
		TimeZone: envOrDefault("TZ", "UTC"),
	}, nil
}

func envOrDefault(name, fallback string) string {
	if value := os.Getenv(name); value != "" {
		return value
	}
	return fallback
}
