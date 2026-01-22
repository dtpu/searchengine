package structs

import (
	"context"
	"time"
    "fmt"
    "errors"
    "github.com/nats-io/nats.go/jetstream"
    "github.com/nats-io/nats.go"
)

const (
    stream_name    = "CRAWL_QUEUE"
    subject_prefix = "crawl.urls"
    consumer_name  = "crawler-worker"
    iter_buffer    = 1000
)

type UrlQueue struct {
    nc        *nats.Conn
    js        jetstream.JetStream
    stream    jetstream.Stream
    consumer  jetstream.Consumer
    iter      jetstream.MessagesContext
    ctx       context.Context
    cancel    context.CancelFunc
}

func InitializeQueue() (*UrlQueue, error) {
    ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)

	nc, err := nats.Connect(nats.DefaultURL)
    if err != nil {
        cancel()
        return nil, fmt.Errorf("failed to connect to NATS: %w", err)
    }
    js, err := jetstream.New(nc)
    if err != nil {
        cancel()
        nc.Close()
        return nil, fmt.Errorf("failed to create JetStream context: %w", err)
    }
    
    st, err := js.Stream(ctx, stream_name)
    if err != nil {
        // Stream doesn't exist, create it
        st, err = js.CreateStream(ctx, jetstream.StreamConfig{
            Name:     stream_name,
            Subjects: []string{subject_prefix + ".*"},
            Retention: jetstream.WorkQueuePolicy, // for work queue
            Storage:   jetstream.FileStorage,     // persist
        })
        if err != nil {
            cancel()
            nc.Close()
            return nil, fmt.Errorf("failed to create stream: %w", err)
        }
    }
    
    c, err := st.CreateOrUpdateConsumer(ctx, jetstream.ConsumerConfig{
        Durable:   "CONS",
        AckPolicy: jetstream.AckExplicitPolicy,
    })
    if err != nil {
        cancel()
        nc.Close()
        return nil, fmt.Errorf("failed to create consumer: %w", err)
    }

    iter, err := c.Messages(jetstream.PullMaxMessages(iter_buffer))
    if err != nil {
        cancel()
        nc.Close()
        return nil, fmt.Errorf("failed to create message iterator: %w", err)
    }

    defer func () {
        cancel()
        iter.Drain()
        iter.Stop()
        js.CleanupPublisher()
        nc.Drain()
        nc.Close()
    }()
    
    return &UrlQueue{
        nc:       nc,
        js:       js,
        stream:   st,
        consumer: c,
        iter:     iter,
        ctx:      ctx,
        cancel:   cancel,
    }, nil
}

func (uq *UrlQueue) Enqueue(url string) error {
    if uq.ctx.Err() != nil {
        return errors.New("queue is closed")
    }
    
    ctx, cancel := context.WithTimeout(uq.ctx, 5*time.Second)
    defer cancel()
    
    _, err := uq.js.Publish(ctx, subject_prefix+".new", []byte(url))
    if err != nil {
        return fmt.Errorf("failed to enqueue URL: %w", err)
    }
    return nil
}

func (uq *UrlQueue) EnqueueBatch(urls []string) error {
    if uq.ctx.Err() != nil {
        return errors.New("queue is closed")
    }
    
    for _, url := range urls {
        // Use async publish for better performance
        _, err := uq.js.PublishAsync(subject_prefix+".new", []byte(url))
        if err != nil {
            return fmt.Errorf("failed to enqueue URL %s: %w", url, err)
        }
    }
    return nil
}

// this is blocking
func (uq *UrlQueue) Dequeue() (jetstream.Msg, error) {
    if uq.ctx.Err() != nil {
        return nil, errors.New("queue is closed")
    }
    
    msg, err := uq.iter.Next()
    if err != nil {
        return nil, fmt.Errorf("failed to get next message: %w", err)
    }
    
    return msg, nil
}

func (uq *UrlQueue) QueueSize() (uint64, error) {
    info, err := uq.consumer.Info(uq.ctx)
    if err != nil {
        return 0, fmt.Errorf("failed to get consumer info: %w", err)
    }
    
    return info.NumPending, nil
}

func (uq *UrlQueue) Close() error {
    uq.cancel()
    
    if uq.iter != nil {
        uq.iter.Stop()
    }
    
    if uq.nc != nil {
        if err := uq.nc.Drain(); err != nil {
            uq.nc.Close()
            return fmt.Errorf("failed to drain connection: %w", err)
        }
    }
    
    return nil
}
