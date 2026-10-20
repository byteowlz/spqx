# spqx

Local speech synthesis from text and a reference voice. Execution engines and voice conditioning remain distinct from conversation identity and authority.

## Language

**Engine**:
A complete text-and-reference-to-audio implementation.
_Avoid_: Tensor backend when referring to the complete synthesis pipeline.

**Device**:
A compute resource selected within an engine, such as a CPU or a particular GPU.
_Avoid_: Engine when only the hardware target changes.

**Voice reference**:
Audio and its matching transcript used to condition synthesis.
_Avoid_: Enrollment, identity proof, or permission to redistribute.

**Prepared voice session**:
A loaded model and checkpoint-scoped reference conditioning retained for repeated synthesis requests.
_Avoid_: Conversation session, durable session, cross-engine voice embedding.

**Synthesis request**:
One text utterance with generation options and its own output/cancellation identity.
_Avoid_: Conversation turn or delegated task.

**Speech cancellation**:
Ending a synthesis request and fencing its remaining audio, without cancelling an independent delegated task.

**Worker**:
A process retaining a prepared voice session and serving synthesis requests.

**Portable runtime**:
The executable and matching engine components needed for a particular operating system and architecture.
_Avoid_: Universal support when only compilation or an upstream capability is known.
