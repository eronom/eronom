from fastapi import FastAPI
import feedparser
import httpx

app = FastAPI()

@app.get("/news")
async def get_news():
    # Fetch TechCrunch or BBC RSS feed using httpx asynchronously, then parse it
    rss_url = "https://feeds.bbci.co.uk/news/world/rss.xml"
    
    async with httpx.AsyncClient() as client:
        response = await client.get(rss_url)
    
    # Parse the XML feed data using feedparser
    feed = feedparser.parse(response.text)
    
    # Format feed entries for your frontend
    articles = []
    for entry in feed.entries[:8]:  # Get the top 8 articles
        articles.append({
            "title": entry.get("title", ""),
            "summary": entry.get("summary", ""),
            "link": entry.get("link", ""),
            "published": entry.get("published", "")
        })
        
    return {
        "feed_title": feed.feed.get("title", "News Feed"),
        "articles": articles
    }
