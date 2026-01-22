// Example using net/http, goroutines, and channels
package main

import (
	"fmt"
	"io"
	"net/http"
	"sync"
)

func fetchURL(url string, wg *sync.WaitGroup, ch chan<- string) {
	defer wg.Done()
	resp, err := http.Get(url)
	if err != nil {
		ch <- fmt.Sprintf("Error fetching %s: %v", url, err)
		return
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	ch <- fmt.Sprintf("Success %s: %d bytes", url, len(body))
}

func main() {
	urls := []string{"https://example.com", "https://danielpu.dev"}
	var wg sync.WaitGroup
	results := make(chan string, len(urls))

	for _, url := range urls {
		wg.Add(1)
		go fetchURL(url, &wg, results)
	}

	wg.Wait()
	close(results)

	for result := range results {
		fmt.Println(result)
	 }
}
