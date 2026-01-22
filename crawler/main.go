// Example using net/http, goroutines, and channels
package main

import (
	"fmt"
	"log"
	"net/http"
	"github.com/dtpu/searchengine/crawler/structs"
	"github.com/dtpu/searchengine/crawler/parser"
	"github.com/nats-io/nats.go/jetstream"
)

const NUM_WORKERS = 50
var q *structs.UrlQueue

func crawl(url string) error {
	resp, err := http.Get(url)
	if err != nil {
		return err
	}

	parsedHTML, err := parser.ParseHTML(resp.Body, url)
	if err != nil {
		return err
	}
	for _, link := range parsedHTML.Links {
		print(link, "\n")
		err := q.Enqueue(link)
		if err != nil {
			log.Println("Failed to enqueue link:", link, err)
		}
	}

	defer resp.Body.Close()
	return nil
}

func main() {
	q, err := structs.InitializeQueue("nats://localhost:4222")
	if err != nil {
        panic(err)
    }
    defer q.Close()

	print("Queue size: ", q.QueueSize(), "\n")

    
    // Seed initial URLs
    q.EnqueueBatch([]string{
        "https://example.com",
        "https://danielpu.dev",
    })
	
    // Start workers
    sem := make(chan struct{}, NUM_WORKERS)
    
    for {
        sem <- struct{}{} // wait for worker slot
        

        msg, err := q.Dequeue()
        if err != nil {
            <-sem
            log.Println("Error dequeuing:", err)
            continue
        }
        
        go func(msg jetstream.Msg) {
            defer func() { <-sem }()

			url := string(msg.Data())
            
            // Crawl the URL
            if err := crawl(url); err != nil {
                log.Println("Crawl failed:", url, err)
                msg.Nak() // requeue
            } else {
                fmt.Println("Crawled:", url)
                msg.Ack() // success
            }
        }(msg)
    }

	

}
