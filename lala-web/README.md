# LalaSearch Web Frontend

A retro 1990s-style web interface for LalaSearch built with plain HTML, CSS, and Alpine.js.

## Features

- 🎨 **Retro Design**: Authentic 1990s aesthetic with beveled buttons and classic colors
- ⚡ **Alpine.js**: Lightweight interactivity without heavy build tools or npm dependencies
- 🔍 **Search Interface**: Clean search box with real-time result display
- 📄 **Pagination**: Navigate through search results with previous/next buttons
- 🔗 **URL Integration**: Click to open results, copy URLs to clipboard
- 📱 **Responsive**: Works on desktop and mobile browsers
- 🚀 **Nginx Served**: Fast static file serving with API proxy

## Architecture

```
lala-web/
├── index.html          # Single HTML file with all styles and Alpine.js logic
├── nginx.conf          # Nginx configuration for routing
└── Dockerfile          # Container image for the web service
```

## How It Works

1. **Frontend** (Nginx on port 8080):
   - Serves the static `index.html` file
   - Handles all client-side state with Alpine.js
   - No build process, no npm, no frameworks

2. **API Proxy**:
   - Routes `/api/search` requests to `lala-agent:3000/search`
   - Handles CORS and necessary headers
   - Transparent to the frontend

3. **Backend Integration**:
   - Sends search queries to `POST /search` endpoint
   - Expects response: `{ results: [...], total: number }`
   - Displays results in paginated format

## Running

Start all services including the web frontend:

```bash
docker-compose up -d --build
```

Access the web UI at: **http://localhost:8080**

## Technologies

- **HTML5**: Semantic markup
- **CSS3**: Retro styling with flexbox
- **Alpine.js 3.x**: Lightweight interactivity (loaded from CDN)
- **Nginx**: Reverse proxy and static serving
- **Docker**: Container deployment

## No External Dependencies

This frontend has **zero npm dependencies**:
- ✅ No webpack, parcel, vite builds
- ✅ No node_modules
- ✅ No transpilation needed
- ✅ Alpine.js loaded directly from CDN

## Customization

### Change Port
Edit `docker-compose.yml`:
```yaml
ports:
  - "8000:80"  # Change 8000 to your desired port
```

### Modify Styling
Edit the `<style>` section in `index.html`:
- Background colors
- Button styles
- Font choices
- Layout spacing

### Adjust Search Features
Edit the `searchApp()` Alpine component:
- Pagination limit
- API endpoint
- Result formatting
- Error handling

## Features Detail

### Search
1. Type or paste a query
2. Press Enter or click SEARCH button
3. Results load with pagination

### Result Display
- **Title**: Click to open in new tab
- **URL**: Click to copy to clipboard
- **Domain**: Source website
- **Score**: Relevance percentage
- **Excerpt**: First 500 characters of content

### Pagination
- 10 results per page (configurable)
- Previous/Next buttons
- Current page indicator
- Smooth scroll to top on page change

## Performance

- **Page Load**: < 100ms (static HTML only)
- **Search**: Depends on backend (typically < 200ms)
- **Size**: ~40KB HTML file, Alpine.js ~15KB from CDN
- **No Build Time**: Change HTML, refresh browser, see changes immediately

## Development

To modify the interface:

1. Edit `lala-web/index.html`
2. Rebuild container: `docker-compose build lala-web`
3. Restart service: `docker-compose up -d lala-web`
4. Or mount as volume for live editing:

```yaml
volumes:
  - ./lala-web/index.html:/usr/share/nginx/html/index.html
```

## Browser Support

Works in all modern browsers:
- Chrome/Edge 60+
- Firefox 55+
- Safari 11+
- Mobile browsers (iOS Safari, Chrome Android)

## License

SPDX-License-Identifier: BSD-3-Clause
Copyright (c) 2026 Aleksandr Ptakhin
