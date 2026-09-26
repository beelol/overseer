package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"time"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
)

// This local service fixture exports sanitized incident evidence. No production
// endpoint is contacted; each invocation creates and removes its own schema.
type Bundle struct {
	Version int    `json:"fixtureVersion"`
	Window  string `json:"window"`
	Request struct {
		TraceID    string `json:"traceId"`
		TimedOut   bool   `json:"timedOut"`
		PoolWaitMS int64  `json:"poolWaitMs"`
		At         string `json:"at"`
	} `json:"request"`
	SQL struct {
		TraceID          string `json:"traceId"`
		OpenTransactions int    `json:"openTransactions"`
		Source           string `json:"source"`
		Handler          string `json:"handler"`
		At               string `json:"at"`
	} `json:"sql"`
	Queue struct {
		TraceID    string `json:"traceId"`
		Deliveries int    `json:"deliveries"`
		Handler    string `json:"handler"`
		At         string `json:"at"`
	} `json:"queue"`
	Conclusion string `json:"conclusion"`
}

func consumeWithRetry(ctx context.Context, pool *pgxpool.Pool, ready chan<- struct{}, release <-chan struct{}) error {
	tx, err := pool.Begin(ctx)
	if err != nil {
		return err
	}
	defer tx.Rollback(ctx)
	var id int
	// The retry wait below holds a transaction and therefore the pool's only
	// connection. This is the seeded defect under investigation.
	if err := tx.QueryRow(ctx, `SELECT id FROM deliveries ORDER BY id LIMIT 1`).Scan(&id); err != nil {
		return err
	}
	ready <- struct{}{}
	<-release
	_, err = tx.Exec(ctx, `DELETE FROM deliveries WHERE id=$1`, id)
	if err != nil {
		return err
	}
	return tx.Commit(ctx)
}

func Generate(ctx context.Context, dsn string) (Bundle, error) {
	var out Bundle
	admin, err := pgx.Connect(ctx, dsn)
	if err != nil {
		return out, err
	}
	defer admin.Close(ctx)
	schema := fmt.Sprintf("dispatch_%d", time.Now().UnixNano())
	if _, err := admin.Exec(ctx, `CREATE SCHEMA `+schema); err != nil {
		return out, err
	}
	defer admin.Exec(context.Background(), `DROP SCHEMA `+schema+` CASCADE`)
	cfg, err := pgxpool.ParseConfig(dsn)
	if err != nil {
		return out, err
	}
	cfg.MaxConns = 1
	cfg.ConnConfig.RuntimeParams["search_path"] = schema
	cfg.ConnConfig.RuntimeParams["application_name"] = schema
	pool, err := pgxpool.NewWithConfig(ctx, cfg)
	if err != nil {
		return out, err
	}
	defer pool.Close()
	if _, err := pool.Exec(ctx, `CREATE TABLE deliveries(id integer PRIMARY KEY, trace_id text NOT NULL)`); err != nil {
		return out, err
	}
	if _, err := pool.Exec(ctx, `INSERT INTO deliveries VALUES (1,'ship-trace-021'),(2,'ship-trace-021')`); err != nil {
		return out, err
	}
	ready := make(chan struct{})
	release := make(chan struct{})
	done := make(chan error, 1)
	go func() { done <- consumeWithRetry(ctx, pool, ready, release) }()
	select {
	case <-ready:
	case err := <-done:
		return out, fmt.Errorf("first delivery failed before ready: %w", err)
	}
	var openTx int
	if err := admin.QueryRow(ctx, `SELECT count(*) FROM pg_stat_activity WHERE application_name=$1 AND state='idle in transaction'`, schema).Scan(&openTx); err != nil {
		close(release)
		<-done
		return out, err
	}
	requestCtx, cancel := context.WithTimeout(ctx, 50*time.Millisecond)
	started := time.Now()
	var count int
	requestErr := pool.QueryRow(requestCtx, `SELECT count(*) FROM deliveries`).Scan(&count)
	waitMS := time.Since(started).Milliseconds()
	cancel()
	close(release)
	if err := <-done; err != nil {
		return out, err
	}
	if !errors.Is(requestErr, context.DeadlineExceeded) {
		return out, fmt.Errorf("expected request pool timeout, got %v", requestErr)
	}
	secondReady := make(chan struct{})
	secondRelease := make(chan struct{})
	go func() { done <- consumeWithRetry(ctx, pool, secondReady, secondRelease) }()
	select {
	case <-secondReady:
	case err := <-done:
		return out, fmt.Errorf("redelivery failed before ready: %w", err)
	}
	close(secondRelease)
	if err := <-done; err != nil {
		return out, err
	}
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM deliveries`).Scan(&count); err != nil {
		return out, err
	}
	if count != 0 {
		return out, fmt.Errorf("expected both queue deliveries to complete, found %d", count)
	}
	out.Version = 1
	out.Window = "10:00-10:15"
	out.Request.TraceID = "ship-trace-021"
	out.Request.TimedOut = true
	out.Request.PoolWaitMS = waitMS
	out.Request.At = "10:07:14"
	out.SQL.TraceID = out.Request.TraceID
	out.SQL.OpenTransactions = openTx
	out.SQL.Source = "shipment.go:consumeWithRetry"
	out.SQL.Handler = "consumeWithRetry"
	out.SQL.At = "10:07:13"
	out.Queue.TraceID = out.Request.TraceID
	out.Queue.Deliveries = 2
	out.Queue.Handler = "consumeWithRetry"
	out.Queue.At = "10:07:12,10:07:15"
	out.Conclusion = "correlated; causal mechanism plausible but not proven by exported logs"
	return out, nil
}

func main() {
	dsn := os.Getenv("DISPATCH_DATABASE_URL")
	if dsn == "" {
		fmt.Fprintln(os.Stderr, "DISPATCH_DATABASE_URL is required")
		os.Exit(2)
	}
	bundle, err := Generate(context.Background(), dsn)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if err := json.NewEncoder(os.Stdout).Encode(bundle); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
