The overall project idea is that it's social open search. We keep small management core on our side, but resources on which we allocate crawling, indexing and storing/querying process are unstable and not in our infrastructure.

We authentificate these nodes and give them tasks from agents started as management ones. So we need to understand which architecture should better here. For draft I thought keeping management tasks in the queue (need to choose one better suiting this) and search queries do through http.

We need also to run some helper scripts to issue letsencrypt certificates for domains. Domains of 3rd level we can provide on our side, but need to make checks and issue cert.

We'll need to keep information which sites or parts of documents are on which machine in the future. But maybe for now we can call just all search machines, when user comes to main website to search for information.

// the one current mode to postpone many critical interactions questions
agent --mode all

// todo
agent --mode manager
agent --mode serve
agent --mode worker

Need crawler which familiar with rules. Even if we start with a few internet websites, might be not good to be banned and do this properly

We'll need to choose database for crawled/indexed information. No need in transactions support. Need possible massive scaling and fast insertion/updating/querying. **Apache Cassandra** - open source, CQL-compatible, horizontal scaling.

crawl statistics. clickhouse?

umami for statistics. GDPR, no cookies, no consent.
Where to put LLM summary. Which one to use. Very small cpu self-hosted?

allowed domains list not to stuck into the legal, adult and other complience from day one. Also we have no resources for all these stuff.

wikipedia, open books, mastodon?

wikipedia 7.000.000 eng pages. Up to 150 Kb.
Need compression and Cassandra might not be the best for these big blobs storages. S3, Storage accounts?
Cheap solutions or own.
https://copilot.microsoft.com/shares/A8RV1y7Y9QZYe2ZFRfEt6
Also if we start to index images? (not now)