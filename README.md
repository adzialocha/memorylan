# memorylan

MemoryLAN is "a Local Area Content Replication Mesh" by Christian Tschudin, University of Basel.
This is a Rust implementation of MemoryLAN using a Cuckoo Filter.

A MemoryLAN consists of interconnected memory switches that aim at maintaining LAN-wide coherent
cache content. The protocol is based on the following two techniques: fast push with loop
prevention, and cache assimilation with mitigation of NACK implosion (slow repair).

In a nutshell, the system works by flooding new memory pages. Loops are prevented by a fixed-size
FIFO deny-list (history) of re cently forwarded packets. In parallel, new memory pages enter at the
head of a fixed-size FIFO content queue (cache) and old cache entries are automatically evicted in
that process.

A fingerprint (Cuckoo filter) of a switch's cache is periodically broadcast such that neighbors can
detect cache discrepancies. Neighbors send curtesy copies of missing content, modulated by
node-density in order to prevent a NACK implosion. The reception of a memory page that is already
present in the cache leads to moving that page to the front of the queue i.e., to "keep this content
hot" and to let the distributed cache converge towards a coherent subset of shared content.
