{-# OPTIONS --safe #-}
-- Runtime.agda — formal invariants of the Sovereign Agent Tool
-- Runtime, self-contained (Agda builtins only, no stdlib).
--
-- The properties proved here are the by-construction ones: the
-- model makes violations unrepresentable, and Agda checks the
-- lemmas. The topological-order development lives in Topo.agda.
-- Nothing in this file is a postulate.

module Runtime where

open import Agda.Builtin.Equality using (_≡_; refl)
open import Agda.Builtin.Bool using (Bool; true; false)
open import Agda.Builtin.Nat using (Nat; zero; suc)
open import Agda.Builtin.List using (List; []; _∷_)

-- ---------- basics ----------
data ⊥ : Set where
data ⊤ : Set where
  tt : ⊤

record Σ (A : Set) (B : A → Set) : Set where
  constructor _,_
  field
    fst : A
    snd : B fst
open Σ

_×_ : Set → Set → Set
A × B = Σ A (λ _ → B)

data Maybe (A : Set) : Set where
  just : A → Maybe A
  nothing : Maybe A

cong : {A B : Set} (f : A → B) {x y : A} → x ≡ y → f x ≡ f y
cong f refl = refl

-- decidable equality on Nat (identifiers are registry indices)
_≟_ : Nat → Nat → Bool
zero ≟ zero = true
suc a ≟ suc b = a ≟ b
_ ≟ _ = false

≟-refl : (a : Nat) → (a ≟ a) ≡ true
≟-refl zero = refl
≟-refl (suc a) = ≟-refl a

≟-sound : (a b : Nat) → (a ≟ b) ≡ true → a ≡ b
≟-sound zero zero _ = refl
≟-sound (suc a) (suc b) p = cong suc (≟-sound a b p)

-- membership
data _∈_ {A : Set} (x : A) : List A → Set where
  here : {xs : List A} → x ∈ (x ∷ xs)
  there : {y : A} {xs : List A} → x ∈ xs → x ∈ (y ∷ xs)

-- ---------- capabilities and the registry ----------
record Capability : Set where
  field
    id : Nat            -- registry identifier
    policyClass : Nat   -- index of its permission class
open Capability

Registry : Set
Registry = List Capability

ids : Registry → List Nat
ids [] = []
ids (c ∷ r) = id c ∷ ids r

-- Property 7: registry identifiers are unique, and inserting a
-- capability with a fresh identifier preserves uniqueness.
Unique : List Nat → Set
Unique [] = ⊤
Unique (x ∷ xs) = (x ∈ xs → ⊥) × Unique xs

insert-fresh : (c : Capability) (r : Registry) →
               (id c ∈ ids r → ⊥) → Unique (ids r) → Unique (ids (c ∷ r))
insert-fresh c r fresh unique-r = (fresh , unique-r)

-- ---------- authorization (Properties 4 and 5) ----------
-- An Execution is a dependent record: it can only be built from
-- a capability that is registered, whose policy check holds,
-- and whose schema validation holds. There is no other way to
-- construct one. That is the proof.
record Execution (r : Registry)
                 (policy : Capability → Bool)
                 (validate : Capability → Bool) : Set where
  field
    cap : Capability
    registered : cap ∈ r
    policy-ok : policy cap ≡ true
    schema-ok : validate cap ≡ true
open Execution

-- The gate: witnesses in, execution out. Without all three
-- witnesses the call does not typecheck.
authorize : {r : Registry} {p v : Capability → Bool} (c : Capability) →
            c ∈ r → p c ≡ true → v c ≡ true → Execution r p v
authorize c reg pol sch = record
  { cap = c ; registered = reg ; policy-ok = pol ; schema-ok = sch }

-- Corollaries: every execution is registered and gated.
execution-registered : {r : Registry} {p v : Capability → Bool} →
                       (e : Execution r p v) → cap e ∈ r
execution-registered e = registered e

execution-policy-ok : {r : Registry} {p v : Capability → Bool} →
                      (e : Execution r p v) → p (cap e) ≡ true
execution-policy-ok e = policy-ok e

execution-schema-ok : {r : Registry} {p v : Capability → Bool} →
                      (e : Execution r p v) → v (cap e) ≡ true
execution-schema-ok e = schema-ok e

-- ---------- discovery (Property 8) ----------
-- A capability enters only through the verified branch of a
-- discovery result. The failed branch carries an error code and
-- no capability; extraction from it is nothing, by definition.
data DiscoveryResult (C : Set) : Set where
  verified : C → DiscoveryResult C
  failed : Nat → DiscoveryResult C

extract : {C : Set} → DiscoveryResult C → Maybe C
extract (verified c) = just c
extract (failed _) = nothing

failure-yields-nothing : {C : Set} (code : Nat) →
                         extract (failed {C} code) ≡ nothing
failure-yields-nothing _ = refl

-- Property 2 (by construction): an executable graph node IS an
-- Execution, so it references a registered capability — the
-- pairing cannot be formed any other way.
ExecutableNode : Registry → (Capability → Bool) → (Capability → Bool) → Set
ExecutableNode r p v = Execution r p v

node-capability : {r : Registry} {p v : Capability → Bool} →
                  ExecutableNode r p v → Capability
node-capability = cap

node-registered : {r : Registry} {p v : Capability → Bool} →
                  (nd : ExecutableNode r p v) → node-capability nd ∈ r
node-registered nd = registered nd
