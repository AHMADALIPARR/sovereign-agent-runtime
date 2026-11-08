{-# OPTIONS --safe #-}
-- Topo.agda — Properties 1 and 3: a valid acyclic execution
-- graph admits a topological ordering, and in that ordering
-- every dependency precedes its dependent node.
--
-- Acyclicity is witnessed by a ranking ρ : Nat → Nat with
-- ρ u < ρ v for every edge (u , v). The ordering is insertion
-- sort by ρ over the node list; the executor runs the list in
-- order, so "Before u v L" IS "u executes before v".
-- A Graph in this model is well-formed by construction: its
-- own field certifies that every edge's endpoints are nodes.
-- Self-contained; checked by Agda. No postulates.

module Topo where

open import Agda.Builtin.Equality using (_≡_; refl)
open import Agda.Builtin.Bool using (Bool; true; false)
open import Agda.Builtin.Nat using (Nat; zero; suc)
open import Agda.Builtin.List using (List; []; _∷_)
open import Runtime using (⊥; ⊤; tt; Σ; _,_; _×_; _∈_; here; there)

data _⊎_ (A B : Set) : Set where
  inj₁ : A → A ⊎ B
  inj₂ : B → A ⊎ B

⊥-elim : {A : Set} → ⊥ → A
⊥-elim ()

sym : {x y : Nat} → x ≡ y → y ≡ x
sym refl = refl

subst : (P : Nat → Set) {x y : Nat} → x ≡ y → P x → P y
subst P refl px = px

∈-empty : {x : Nat} → x ∈ [] → ⊥
∈-empty ()

-- ---------- order on Nat ----------
data _≤_ : Nat → Nat → Set where
  z≤n : {n : Nat} → zero ≤ n
  s≤s : {a b : Nat} → a ≤ b → suc a ≤ suc b

_<_ : Nat → Nat → Set
a < b = suc a ≤ b

≤-refl : (a : Nat) → a ≤ a
≤-refl zero = z≤n
≤-refl (suc a) = s≤s (≤-refl a)

≤-trans : {a b c : Nat} → a ≤ b → b ≤ c → a ≤ c
≤-trans z≤n _ = z≤n
≤-trans (s≤s p) (s≤s q) = s≤s (≤-trans p q)

suc≰ : {n : Nat} → suc n ≤ n → ⊥
suc≰ {suc n} (s≤s p) = suc≰ p

<≰ : {a b : Nat} → a < b → b ≤ a → ⊥
<≰ lt ge = suc≰ (≤-trans lt ge)

data Dec (P : Set) : Set where
  yes : P → Dec P
  no : (P → ⊥) → Dec P

≤-total : (a b : Nat) → (a ≤ b) ⊎ (b ≤ a)
≤-total zero _ = inj₁ z≤n
≤-total (suc a) zero = inj₂ z≤n
≤-total (suc a) (suc b) with ≤-total a b
... | inj₁ p = inj₁ (s≤s p)
... | inj₂ p = inj₂ (s≤s p)

≤-dec : (a b : Nat) → Dec (a ≤ b)
≤-dec zero _ = yes z≤n
≤-dec (suc _) zero = no (λ ())
≤-dec (suc a) (suc b) with ≤-dec a b
... | yes p = yes (s≤s p)
... | no np = no (λ { (s≤s p) → np p })

¬≤→≥ : {a b : Nat} → (a ≤ b → ⊥) → b ≤ a
¬≤→≥ {a} {b} np with ≤-total a b
... | inj₁ p = ⊥-elim (np p)
... | inj₂ p = p

-- ---------- insertion sort by a ranking ρ ----------
insert : (ρ : Nat → Nat) → Nat → List Nat → List Nat
insert ρ x [] = x ∷ []
insert ρ x (h ∷ hs) with ≤-dec (ρ x) (ρ h)
... | yes _ = x ∷ h ∷ hs
... | no _ = h ∷ insert ρ x hs

sort : (ρ : Nat → Nat) → List Nat → List Nat
sort ρ [] = []
sort ρ (x ∷ xs) = insert ρ x (sort ρ xs)

-- sortedness, all-bound form: the head's rank bounds the tail
data Sorted (ρ : Nat → Nat) : List Nat → Set where
  nil : Sorted ρ []
  cons : {x : Nat} {l : List Nat} →
         ((z : Nat) → z ∈ l → ρ x ≤ ρ z) → Sorted ρ l → Sorted ρ (x ∷ l)

sorted-tail : {ρ : Nat → Nat} {x : Nat} {l : List Nat} →
              Sorted ρ (x ∷ l) → Sorted ρ l
sorted-tail (cons _ s) = s

head-bound : {ρ : Nat → Nat} {x z : Nat} {l : List Nat} →
             Sorted ρ (x ∷ l) → z ∈ l → ρ x ≤ ρ z
head-bound (cons bound _) mem = bound _ mem

-- membership through insert, both directions
∈-insert-out : {ρ : Nat → Nat} {x y : Nat} {l : List Nat} →
               x ∈ insert ρ y l → (x ≡ y) ⊎ (x ∈ l)
∈-insert-out {l = []} here = inj₁ refl
∈-insert-out {ρ} {x} {y} {h ∷ hs} mem with ≤-dec (ρ y) (ρ h)
∈-insert-out here | yes _ = inj₁ refl
∈-insert-out (there mem) | yes _ = inj₂ mem
∈-insert-out here | no _ = inj₂ here
∈-insert-out (there mem) | no _ with ∈-insert-out mem
... | inj₁ p = inj₁ p
... | inj₂ p = inj₂ (there p)

∈-insert-in : {ρ : Nat → Nat} {x y : Nat} {l : List Nat} →
              x ∈ l → x ∈ insert ρ y l
∈-insert-in {ρ} {x} {y} {h ∷ hs} mem with ≤-dec (ρ y) (ρ h)
... | yes _ = there mem
... | no _ with mem
...   | here = here
...   | there rest = there (∈-insert-in rest)

∈-insert-head : {ρ : Nat → Nat} {y : Nat} {l : List Nat} → y ∈ insert ρ y l
∈-insert-head {l = []} = here
∈-insert-head {ρ} {y} {h ∷ hs} with ≤-dec (ρ y) (ρ h)
... | yes _ = here
... | no _ = there (∈-insert-head {ρ} {y} {hs})

∈-sort : {ρ : Nat → Nat} {x : Nat} {l : List Nat} → x ∈ l → x ∈ sort ρ l
∈-sort {ρ} {l = a ∷ as} here = ∈-insert-head {ρ} {a} {sort ρ as}
∈-sort {ρ} {l = a ∷ as} (there mem) = ∈-insert-in {ρ} {_} {a} {sort ρ as} (∈-sort mem)

-- insertion preserves sortedness
insert-sorted : {ρ : Nat → Nat} (x : Nat) {l : List Nat} →
                Sorted ρ l → Sorted ρ (insert ρ x l)
insert-sorted x {[]} _ = cons (λ z mem → ⊥-elim (∈-empty mem)) nil
insert-sorted {ρ} x {h ∷ hs} (cons bound sorted-hs) with ≤-dec (ρ x) (ρ h)
... | yes p = cons bound' (cons bound sorted-hs)
  where
    bound' : (z : Nat) → z ∈ (h ∷ hs) → ρ x ≤ ρ z
    bound' _ here = p
    bound' _ (there mem) = ≤-trans p (bound _ mem)
... | no np = cons bound'' (insert-sorted x sorted-hs)
  where
    bound'' : (z : Nat) → z ∈ insert ρ x hs → ρ h ≤ ρ z
    bound'' _ mem with ∈-insert-out mem
    ... | inj₁ eq = subst (λ w → ρ h ≤ ρ w) (sym eq) (¬≤→≥ np)
    ... | inj₂ in-hs = bound _ in-hs

sort-sorted : {ρ : Nat → Nat} (l : List Nat) → Sorted ρ (sort ρ l)
sort-sorted [] = nil
sort-sorted {ρ} (x ∷ xs) = insert-sorted {ρ} x {sort ρ xs} (sort-sorted xs)

-- ---------- Before: positional precedence ----------
data Before (x y : Nat) : List Nat → Set where
  first : {xs : List Nat} → y ∈ xs → Before x y (x ∷ xs)
  skip : {z : Nat} {xs : List Nat} → Before x y xs → Before x y (z ∷ xs)

before-≤ : {ρ : Nat → Nat} {x y : Nat} {l : List Nat} →
           Sorted ρ l → Before x y l → ρ x ≤ ρ y
before-≤ s (first mem) = head-bound s mem
before-≤ s (skip b) = before-≤ (sorted-tail s) b

∈-cases : {x a : Nat} {xs : List Nat} → x ∈ (a ∷ xs) → (x ≡ a) ⊎ (x ∈ xs)
∈-cases here = inj₁ refl
∈-cases (there mem) = inj₂ mem

-- the heart of it: in a sorted list, strictly smaller rank
-- forces positional precedence
before-of-rank : {ρ : Nat → Nat} {u v : Nat} {l : List Nat} →
                 Sorted ρ l → u ∈ l → v ∈ l → ρ u < ρ v → Before u v l
before-of-rank {l = []} _ ()
before-of-rank {ρ} {u} {v} {a ∷ rest} s u-in v-in lt
  with ∈-cases u-in | ∈-cases v-in
... | inj₁ refl | inj₁ refl = ⊥-elim (<≰ lt (≤-refl (ρ v)))
... | inj₁ refl | inj₂ v-rest = first v-rest
... | inj₂ u-rest | inj₁ refl = ⊥-elim (<≰ lt (head-bound s u-rest))
... | inj₂ u-rest | inj₂ v-rest =
        skip (before-of-rank (sorted-tail s) u-rest v-rest lt)

-- ---------- the graph, well-formed by construction ----------
record Graph : Set where
  field
    nodes : List Nat
    edges : List (Σ Nat (λ _ → Nat))
    wf : (e : Σ Nat (λ _ → Nat)) → e ∈ edges →
         (Σ.fst e ∈ nodes) × (Σ.snd e ∈ nodes)
open Graph

-- Properties 1 and 3 (proved): every ranked (acyclic) graph
-- admits an ordering of all its nodes in which every edge's
-- source precedes its target.
topo-exists : (g : Graph) (ρ : Nat → Nat) →
              ((e : Σ Nat (λ _ → Nat)) → e ∈ edges g →
                ρ (Σ.fst e) < ρ (Σ.snd e)) →
              Σ (List Nat) (λ L →
                ((n : Nat) → n ∈ nodes g → n ∈ L) ×
                ((e : Σ Nat (λ _ → Nat)) → e ∈ edges g →
                  Before (Σ.fst e) (Σ.snd e) L))
topo-exists g ρ ranked =
  (sort ρ (nodes g) , ((λ n mem → ∈-sort mem) , respects))
  where
    respects : (e : Σ Nat (λ _ → Nat)) → e ∈ edges g →
               Before (Σ.fst e) (Σ.snd e) (sort ρ (nodes g))
    respects e e-in =
      before-of-rank (sort-sorted (nodes g))
        (∈-sort (Σ.fst (wf g e e-in)))
        (∈-sort (Σ.snd (wf g e e-in)))
        (ranked e e-in)
