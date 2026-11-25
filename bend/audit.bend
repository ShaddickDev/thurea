# Thurea audit kernel (Bend).
#
# Decides, in parallel, which queried hosts fall under a blocklist rule.
# Hosts and rules arrive as lists of 24-bit label hashes in reverse order
# ("www.example.com" -> [h(com), h(example), h(www)]), so "rule matches host"
# is simply "rule is a prefix of host". Hosts come as a balanced tree of groups,
# and HVM evaluates both halves of every node at the same time.
# Rust (thurea-core::bend) builds the input, runs this, and re-checks every
# hit exactly, so a hash collision can never block a site by mistake.

type QTree:
  Node { ~left, ~right }
  Leaf { bit, labels }
  Group { gid, hosts }

def is_prefix(rule, host):
  match rule:
    case List/Nil:
      return 1
    case List/Cons:
      match host:
        case List/Nil:
          return 0
        case List/Cons:
          if rule.head == host.head:
            return is_prefix(rule.tail, host.tail)
          else:
            return 0

def any_rule(rules, host):
  match rules:
    case List/Nil:
      return 0
    case List/Cons:
      if is_prefix(rules.head, host):
        return 1
      else:
        return any_rule(rules.tail, host)

def concat(a, b):
  match a:
    case List/Nil:
      return b
    case List/Cons:
      return List/Cons(a.head, concat(a.tail, b))

# Blocked hosts as bitmasks: a Group holds up to 24 hosts, a Leaf's `bit` is its place
# in the group (1, 2, 4, ...). The result lists [group, mask] for every group with a hit,
# which keeps HVM's printed result small even when thousands of hosts match.
def hits(tree, rules):
  match tree:
    case QTree/Node:
      return hits(tree.left, rules) | hits(tree.right, rules)
    case QTree/Leaf:
      if any_rule(rules, tree.labels):
        return tree.bit
      else:
        return 0
    case QTree/Group:
      return 0

def audit(tree, rules):
  match tree:
    case QTree/Node:
      return concat(audit(tree.left, rules), audit(tree.right, rules))
    case QTree/Leaf:
      return []
    case QTree/Group:
      mask = hits(tree.hosts, rules)
      if mask == 0:
        return []
      else:
        return [tree.gid, mask]
