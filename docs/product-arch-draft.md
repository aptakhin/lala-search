The overall project idea is that it's social open search. We keep small management core on our side, but resources on which we allocate crawling, indexing and storing/querying process are unstable and not in our infrastructure.

We authentificate these nodes and give them tasks from agents started as management ones. So we need to understand which architecture should better here. For draft I thought keeping management tasks in the queue (need to choose one better suiting this) and search queries do through http.

We need also to run some helper scripts to issue letsencrypt certificates for domains. Domains of 3rd level we can provide on our side, but need to make checks and issue cert.

We'll need to keep information which sites or parts of documents are on which machine in the future. But maybe for now we can call just all search machines, when user comes to main website to search for information.

agent --mode all
agent --mode manager
agent --mode serve
agent --mode worker

Need crawler which familiar with rules. Even if we start with a few internet websites, might be not good to be banned and do this properly

We'll need to choose database for crawled/indexed information. No need in transactions support. Need possible massive scaling and fast insertion/updating/querying. scylladb?

crawl statistics. clickhouse?