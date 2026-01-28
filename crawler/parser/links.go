package parser

import (
	"net/url"
)

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

func GetDomain(link string) (string, error) {
	parsedURL, err := url.Parse(link)
	if err != nil {
		return "", err
	}
	return parsedURL.Hostname(), nil
}
