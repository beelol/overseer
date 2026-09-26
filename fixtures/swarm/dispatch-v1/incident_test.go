package main

import (
	"context"
	"os"
	"testing"
)

func TestIncidentLinksTimedOutRequestToHeldRetryTransaction(t *testing.T) {
	dsn := os.Getenv("DISPATCH_DATABASE_URL")
	if dsn == "" {
		t.Skip("requires disposable PostgreSQL")
	}
	bundle, err := Generate(context.Background(), dsn)
	if err != nil {
		t.Fatal(err)
	}
	if bundle.Version != 1 || bundle.Window != "10:00-10:15" {
		t.Fatalf("wrong bundle identity: %+v", bundle)
	}
	if !bundle.Request.TimedOut || bundle.Request.PoolWaitMS < 35 {
		t.Fatalf("request did not wait on pool: %+v", bundle.Request)
	}
	if bundle.SQL.OpenTransactions != 1 || bundle.SQL.Source != "shipment.go:consumeWithRetry" {
		t.Fatalf("missing held transaction: %+v", bundle.SQL)
	}
	if bundle.Queue.Deliveries != 2 || bundle.Queue.Handler != bundle.SQL.Handler {
		t.Fatalf("redelivery path not linked: %+v", bundle.Queue)
	}
	if bundle.Request.TraceID != bundle.SQL.TraceID || bundle.SQL.TraceID != bundle.Queue.TraceID {
		t.Fatalf("trace chain broken: %+v", bundle)
	}
	if bundle.Conclusion != "correlated; causal mechanism plausible but not proven by exported logs" {
		t.Fatalf("overclaimed: %q", bundle.Conclusion)
	}
}
