# Milona — GenAI Application Architecture

### A model-agnostic pipeline for ingesting, storing, and reasoning over knowledge

Milona ingests documents from multiple sources, normalizes and chunks them, then persists
the result as both a knowledge graph and vector embeddings on a MongoDB-compatible store.
A GenAI application layer sits on top, exposed through an API/CLI/UI presenter, querying
that knowledge via a dedicated API, invoking external tools through MCP/CLI, and routing
inference through a swappable adapter that supports OpenAI, Anthropic, Gemini, DeepSeek,
Qwen, and other LLM providers.

## Diagram
```mermaid
%%{init: {'theme': 'base', 'themeVariables': {'lineColor': '#94a3b8', 'fontFamily': 'inherit'}}}%%
graph TB
    %% --- SECTION INGESTION ---
    subgraph INGEST ["INGEST"]
        direction TB
        Sources[TXT / PDF / Web / ...] --> Pipeline[PIPELINE]
        Pipeline --> Parsing[PARSING]
        Parsing --> Normalize[NORMALIZE]
        Normalize --> Chunk[CHUNK]
        Chunk --> Embed[EMBED or CREATE GRAPH]
        Embed --> Persist[PERSIST]
    end

    %% --- SECTION STOCKAGE (GRAPH & VECTORS) ---
    subgraph STORAGE ["DATABASE"]
        direction TB
        Graph["GRAPH <br> (Compatible: MongoDB / DocumentDB)"]
        Vectors["VECTORS"]
    end
    
    Persist --> Graph

    %% --- SECTION APPLICATION & LOGIQUE CORPS ---
    subgraph APP_LAYER ["GEN AI APPLICATION & INTERFACES"]
        Presenter["PRESENTER: API / CLI / UI"]
        GenAI["APPLICATION: GEN AI"]
        
        %% Flux Presenter <-> GenAI
        Presenter -->|QUESTION| GenAI
        GenAI -->|RESPONSE| Presenter

        %% Bloc Knowledge & API
        subgraph KNOWLEDGE_BLOCK ["KNOWLEDGE"]
            Knowledge[KNOWLEDGE] <--> MongoAPI[MONGO BASED API]
        end
        
        %% Connexions GenAI <-> Knowledge <-> Storage
        GenAI <--> Knowledge
        MongoAPI <--> Graph
        MongoAPI <--> Vectors

        %% Bloc Tools & Externe
        Tools["TOOLS:"]
        GenAI <--> Tools
    end

    %% --- SECTION MCP / CLI ---
    subgraph MCP_BLOCK ["MCP / CLI"]
        MCP["MCP / CLI Area"]
    end
    Tools <--> MCP

    %% --- SECTION MODEL ADAPTER (LLMs) ---
    subgraph MODEL_LAYER ["MODEL ADAPTER"]
        direction LR
        Adapter["ADAPTER"] <--> LLMs["OpenAI <br> Anthropic <br> Gemini <br> DeepSeek <br> Qwen <br> ..."]
    end
    
    GenAI <--> Adapter

    %% Styles pour clarifier le rendu (fond des sous-graphes)
    style INGEST fill:#0b1e3d,stroke:#3b82f6,stroke-width:2px,color:#e0f2fe
    style STORAGE fill:#042f2e,stroke:#14b8a6,stroke-width:2px,color:#ccfbf1
    style APP_LAYER fill:#2e1065,stroke:#8b5cf6,stroke-width:2px,color:#ede9fe
    style KNOWLEDGE_BLOCK fill:#451a03,stroke:#d97706,stroke-width:2px,color:#fef3c7
    style MCP_BLOCK fill:#1e293b,stroke:#64748b,stroke-width:2px,color:#e2e8f0
    style MODEL_LAYER fill:#4c0519,stroke:#e11d48,stroke-width:2px,color:#ffe4e6

    %% Classes des noeuds (fond + texte a fort contraste)
    classDef ingestNode fill:#1d4ed8,stroke:#bfdbfe,stroke-width:1px,color:#ffffff
    classDef storageNode fill:#0f766e,stroke:#99f6e4,stroke-width:1px,color:#ffffff
    classDef appNode fill:#6d28d9,stroke:#ddd6fe,stroke-width:1px,color:#ffffff
    classDef knowledgeNode fill:#b45309,stroke:#fde68a,stroke-width:1px,color:#ffffff
    classDef toolsNode fill:#334155,stroke:#cbd5e1,stroke-width:1px,color:#ffffff
    classDef modelNode fill:#be123c,stroke:#fecdd3,stroke-width:1px,color:#ffffff

    class Sources,Pipeline,Parsing,Normalize,Chunk,Embed,Persist ingestNode
    class Graph,Vectors storageNode
    class Presenter,GenAI appNode
    class Knowledge,MongoAPI knowledgeNode
    class Tools,MCP toolsNode
    class Adapter,LLMs modelNode
```