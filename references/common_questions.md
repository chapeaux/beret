Entering a new codebase is like being dropped into a foreign city without a map. You’re trying to figure out where the "good" neighborhoods are, which alleys to avoid, and why there’s a random statue of a golden retriever in the town square (usually a legacy workaround from 2019).

To help you navigate this, I’ve categorized 100 essential questions every developer asks.

## ---

**1\. High-Level Architecture: The Map**

**Formula:** \[Structural Components\] \+ \[Communication Protocols\] \= System Mental Model

1. What is the primary tech stack (languages, frameworks, versions)?  
2. Is this a monolith, microservices, or a "distributed monolith"?  
3. What is the entry point of the application?  
4. How do different services or modules communicate (REST, gRPC, Pub/Sub)?  
5. Where is the architectural documentation (ADRs, C4 diagrams)?  
6. Is the system event-driven or request-response?  
7. How is the codebase organized (by feature, by layer, or hexagonal)?  
8. What are the external dependencies (APIs, third-party services)?  
9. Where is the "source of truth" for the system’s state?  
10. Are there any deprecated components still in use?

## **2\. Local Environment & Setup: The Mechanics**

**Formula:** \[Hardware Requirements\] \+ \[Dependency Management\] \= Developer Velocity

11. What is the "one-command" setup (e.g., make install, npm run setup)?  
12. Do I need Docker, or can I run this natively?  
13. How are environment variables managed locally?  
14. What version of the runtime (Node, Python, Java) is required?  
15. Is there a "seed" script for the local database?  
16. How do I mock external API dependencies for local dev?  
17. What IDE plugins or extensions are recommended?  
18. Where are the logs located on my local machine?  
19. How do I clear the local cache or build artifacts?  
20. Why did the build just fail on my machine but work on CI?

## **3\. Code Patterns & Style: The Grammar**

**Formula:** \[Linter Rules\] \+ \[Common Idioms\] \= Code Consistency

21. What is the naming convention (camelCase, snake\_case)?  
22. Are we using Functional Programming or OOP principles?  
23. How are errors handled (try/catch, Result types, error codes)?  
24. Where are the shared utility functions or "helpers"?  
25. How do we handle asynchronous operations?  
26. Is there a specific pattern for Dependency Injection?  
27. How are "Constants" or "Magic Numbers" managed?  
28. What is the policy on code comments vs. "self-documenting code"?  
29. How do we handle null or undefined values?  
30. Is there a "Style Guide" document I should read?

## **4\. Business Logic & Domain: The "Why"**

**Formula:** \[User Requirements\] \+ \[Domain Rules\] \= Code Functionality

31. Where does the "core" business logic live?  
32. What are the primary entities (User, Order, Account, etc.)?  
33. How is a "User" uniquely identified across the system?  
34. What are the critical edge cases the code must handle?  
35. Are there complex state machines involved in the logic?  
36. Where are the business rules for pricing/permissions/validation?  
37. How are "Soft Deletes" vs. "Hard Deletes" handled?  
38. What happens when a background job fails?  
39. How is multi-tenancy handled (if applicable)?  
40. Does the code reflect the actual terminology used by the product team?

## **5\. Data & State Management: The Memory**

**Formula:** \[Schema Design\] \+ \[Caching Strategy\] \= Application State

41. What databases are used (SQL, NoSQL, Key-Value)?  
42. Where are the database migration files?  
43. How do I see the current database schema?  
44. How is the application state managed (Redux, Context, Zustand, Server-side)?  
45. Where do we use caching (Redis, Memcached, Browser cache)?  
46. What is the cache invalidation strategy?  
47. Are there any "Heavy" queries I should be aware of?  
48. How do we handle database connection pooling?  
49. Is there any "State" stored in the file system?  
50. How are transactions handled across multiple services?

## **6\. Testing & Quality: The Safety Net**

**Formula:** \[Test Coverage\] \+ \[Mocking Strategy\] \= Deployment Confidence

51. How do I run the unit tests?  
52. Where are the Integration tests?  
53. How do we mock the database in tests?  
54. What is the required test coverage percentage?  
55. How do I run tests for a specific file or function?  
56. Are there End-to-End (E2E) tests (Cypress, Playwright)?  
57. How do we test "Time" (e.g., scheduled events)?  
58. Where are the "snapshots" or "fixtures" stored?  
59. How do I debug a failing test in the CI pipeline?  
60. Is TDD (Test Driven Development) practiced here?

## **7\. Security & Authentication: The Shield**

**Formula:** \[Identity Management\] \+ \[Encryption\] \= Risk Mitigation

61. How are users authenticated (JWT, OAuth, Cookies)?  
62. How is authorization (RBAC/ABAC) implemented?  
63. Where are the API secrets and keys stored?  
64. How do we prevent SQL injection or XSS?  
65. Is PII (Personally Identifiable Information) encrypted at rest?  
66. How do we handle CORS?  
67. Are there any known security vulnerabilities in our dependencies?  
68. How do we rotate secrets?  
69. Who has access to the production database?  
70. Does the codebase comply with GDPR/CCPA?

## **8\. Deployment & CI/CD: The Delivery**

**Formula:** \[Automation\] \+ \[Infrastructure\] \= Production Readiness

71. What happens when I push code to the main branch?  
72. How long does the CI/CD pipeline take?  
73. Where are the build logs for the deployment?  
74. How do we roll back a "bad" deployment?  
75. Is there a staging or "UAT" environment?  
76. How are feature flags managed?  
77. Who is responsible for "Green-lighting" a release?  
78. Is the infrastructure managed via code (Terraform, Pulumi)?  
79. How do we manage different versions of the API?  
80. What is the "On-call" rotation for deployment failures?

## **9\. Performance & Observability: The Pulse**

**Formula:** \[Metrics\] \+ \[Tracing\] \= System Health

81. What are the key performance indicators (KPIs) for this app?  
82. Where are the application logs (ELK, Splunk, Datadog)?  
83. How do we track errors in production (Sentry, Honeybadger)?  
84. Is there distributed tracing for microservices?  
85. What are the current "Bottlenecks" in the system?  
86. How do we handle rate limiting?  
87. What is the "P99" latency for our main endpoints?  
88. Are there any memory leaks we’re currently fighting?  
89. How do we monitor database health?  
90. Where can I see the "Status Dashboard" for the system?

## **10\. Legacy & Team Culture: The Context**

**Formula:** \[Ownership\] \+ \[Historical Knowledge\] \= Collaborative Success

91. Who is the "Subject Matter Expert" for \[X\] module?  
92. Why was \[weird design choice\] made three years ago?  
93. Is there a "Refactoring" backlog?  
94. How often do we do code reviews?  
95. What is the "Definition of Done"?  
96. Where is the most "Fragile" part of the code?  
97. How do we handle technical debt?  
98. What’s the biggest "Pain Point" for the current team?  
99. Is there a Slack channel for \[X\] specific service?  
100. When was the last time this code was actually deployed?

---

That’s the "Century of Questions." If you’re asking these, you’re not just reading code—you’re understanding the ecosystem.