package parser

import (
	"io"
	"log"
	"net/url"
	"github.com/PuerkitoBio/goquery"
)

type ParsedHTML struct {
	Title       string
	Links       []string
	MetaDesc    string
	MetaKeywords []string
}

func isFileLink(link string) bool {
	fileExtensions := []string{
		".jpg", ".jpeg", ".png", ".gif", ".bmp", ".svg",
		".pdf", ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx",
		".zip", ".rar", ".7z", ".tar", ".gz",
		".mp3", ".wav", ".mp4", ".avi", ".mkv",
	}
	for _, ext := range fileExtensions {
		if len(link) >= len(ext) && link[len(link)-len(ext):] == ext {
			return true
		}
	}
	return false
}

func isValidURL(toTest string) bool {
	_, err := url.ParseRequestURI(toTest)
	if err != nil {
		return false
	}

	u, err := url.Parse(toTest)
	if err != nil || !(u.Scheme == "http" || u.Scheme == "https") || u.Host == "" {
		return false
	}
	if isFileLink(u.Path) {
		return false
	}

	return true
}

func ParseHTML(body io.Reader, baseURL string) (*ParsedHTML, error) {
	baseURLParsed, err := url.Parse(baseURL)
	if err != nil {
		log.Println("Error parsing base URL:", err)
		return nil, err
	}
	doc, err := goquery.NewDocumentFromReader(body)
	if err != nil {
		log.Println("Error parsing HTML:", err)
		return nil, err
	}
	links := []string{}
	doc.Find("a").Each(func(i int, s *goquery.Selection) {
		href, exists := s.Attr("href")
		if exists {
			if isValidURL(href) {
				links = append(links, href)
			} else {
				if isValidURL(baseURLParsed.JoinPath(href).String()) { // TODO: this is highkey dumb asf, change later
					links = append(links, baseURLParsed.JoinPath(href).String())
				} else {
					return
				}
			}
		}
	})
	return &ParsedHTML{
		Links: links,
	}, nil
}

