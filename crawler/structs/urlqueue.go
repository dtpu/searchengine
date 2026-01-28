package structs

import (
	"context"
	"time"
    "fmt"
    "errors"
    "github.com/nats-io/nats.go/jetstream"
    "github.com/nats-io/nats.go"
    "github.com/dtpu/searchengine/crawler/parser"
)

const (
    stream_name    = "CRAWL_QUEUE"
    subject_prefix = "url."
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

func InitializeQueue(nats_url string) (*UrlQueue, error) {
    ctx, cancel := context.WithCancel(context.Background()) // Changed from WithTimeout

	nc, err := nats.Connect(nats_url, 
        nats.Timeout(3*time.Second),
        nats.MaxReconnects(3),
        nats.ReconnectWait(1*time.Second),
    )
    if err != nil {
        cancel()
        return nil, fmt.Errorf("failed to connect to NATS: %w", err)
    }
    
    // only for initialization
    initCtx, initCancel := context.WithTimeout(context.Background(), 30*time.Second)
    defer initCancel()
    
    js, err := jetstream.New(nc)
    if err != nil {
        cancel()
        nc.Close()
        return nil, fmt.Errorf("failed to create JetStream context: %w", err)
    }

    st, err := js.Stream(initCtx, stream_name)
    if err != nil {
        // create if not exists (for first run)
        st, err = js.CreateStream(initCtx, jetstream.StreamConfig{
            Name:     stream_name,
            Subjects: []string{subject_prefix + ".*"},
            Retention: jetstream.WorkQueuePolicy,
            Storage:   jetstream.FileStorage,
        })
        if err != nil {
            cancel()
            nc.Close()
            return nil, fmt.Errorf("failed to create stream: %w", err)
        }
    }

    c, err := st.Consumer(initCtx, consumer_name)
    if err != nil {
        // Consumer doesn't exist, create it
        c, err = st.CreateConsumer(initCtx, jetstream.ConsumerConfig{
            Durable:       consumer_name,
            AckPolicy:     jetstream.AckExplicitPolicy,
            FilterSubject: subject_prefix + "*",
        })
        if err != nil {
            cancel()
            nc.Close()
            return nil, fmt.Errorf("failed to create consumer: %w", err)
        }
    }
    
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
    
    domain, err := parser.GetDomain(url)
    if err != nil {
        return fmt.Errorf("failed to get domain from URL: %w", err)
    }
    _, err = uq.js.Publish(ctx, subject_prefix+domain, []byte(url))
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
        domain, err := parser.GetDomain(url)
        if err != nil {
            return fmt.Errorf("failed to get domain from URL %s: %w", url, err)
        }
        _, err = uq.js.PublishAsync(subject_prefix+domain, []byte(url))
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

func (uq *UrlQueue) Empty() (bool) {
    return uq.QueueSize() == 0
}

func (uq *UrlQueue) QueueSize() (uint64) {
    info, err := uq.consumer.Info(uq.ctx)
    if err != nil {
        panic(err) // should not happen
    }
    
    return info.NumPending
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

func (uq *UrlQueue) IsHealthy() bool {
    return uq.nc != nil && uq.nc.IsConnected()
}
