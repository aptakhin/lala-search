# LalaSearch - Distributed Open Source Search Engine

## Vision

LalaSearch is an ambitious open source distributed search engine designed for scalability, resilience, and performance. The architecture leverages a leader-follower agent model where agents can be dynamically promoted or demoted based on system needs.

## Architecture

### Core Components

#### lala-agent
The core agent responsible for:
- **Agent Management**: Coordinating between leader and follower nodes
- **Crawling**: Distributed web crawling with intelligent scheduling
- **Indexing**: Building and maintaining search indices
- **Leader Election**: Dynamic promotion of agents to leader roles
- **Node Coordination**: Managing follower nodes and distributing workload

### Agent Hierarchy

- **Leader Agents**: Coordinate crawling strategies, manage work distribution, and maintain cluster health
- **Follower Agents**: Execute crawling tasks, build local indices, and report to leaders

### Technology Stack

- **Language**: Rust for performance, memory safety, and concurrency
- **Web Framework**: Axum for HTTP services
- **Async Runtime**: Tokio for asynchronous operations
- **Testing**: Built-in Rust testing with TDD approach

## Development Principles

1. **Test-Driven Development (TDD)**: All features start with tests
2. **Code Quality**: Automated linting and formatting
3. **Documentation**: Comprehensive docs for all components
4. **Distributed-First**: Design for horizontal scalability from day one
5. **Open Source**: Community-driven development

## Project Status

🚀 **Early Stage**: Currently bootstrapping the core agent infrastructure.

## Getting Started

See [docs/claude-guidelines.md](claude-guidelines.md) for development workflow and contribution guidelines.
