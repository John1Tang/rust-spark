#!/usr/bin/env bash
# Hands-on demo of rspark against the bundled mock data.
# Run from the repo root: ./examples/demo.sh
set -euo pipefail

CLI="cargo run -q -p rspark-cli --"
DATA=examples/data
EMP="$DATA/employees.csv"
SALES="$DATA/sales.csv"
EVENTS="$DATA/events.json"

hr() { printf '\n\033[1;36m=== %s ===\033[0m\n' "$*"; }
sql() { $CLI sql "$@"; }

hr "1. SELECT * FROM employees  (20 rows, 6 columns)"
sql --input "$EMP" "SELECT * FROM employees"

hr "2. WHERE: high earners in Engineering"
sql --input "$EMP" "SELECT name, salary FROM employees WHERE dept = 'Engineering' AND salary > 90000 ORDER BY salary DESC"

hr "3. GROUP BY: average salary per department"
sql --input "$EMP" "SELECT dept, AVG(salary) AS avg_sal, COUNT(*) AS headcount, MIN(salary) AS min_sal, MAX(salary) AS max_sal FROM employees GROUP BY dept ORDER BY avg_sal DESC"

hr "4. HAVING: departments where avg salary exceeds 80000"
sql --input "$EMP" "SELECT dept, AVG(salary) AS avg_sal FROM employees GROUP BY dept HAVING AVG(salary) > 80000 ORDER BY avg_sal DESC"

hr "5. JOIN: sales attributed by employee id"
sql --input "$EMP" --input "$SALES" "SELECT e.name, s.product, s.amount, s.region FROM employees e JOIN sales s ON e.id = s.id ORDER BY s.amount DESC LIMIT 8"

hr "6. AGGREGATE on joined data: total sales per region"
sql --input "$EMP" --input "$SALES" "SELECT s.region, COUNT(*) AS n_orders, SUM(s.amount) AS total, AVG(s.amount) AS avg_order FROM sales s GROUP BY s.region ORDER BY total DESC"

hr "7. DISTINCT departments"
sql --input "$EMP" "SELECT DISTINCT dept FROM employees ORDER BY dept"

hr "8. LIKE pattern matching"
sql --input "$EMP" "SELECT name, dept FROM employees WHERE name LIKE 'A%' OR name LIKE '%a%'"

hr "9. JSON data source: events"
sql --input-format json --input "$EVENTS" "SELECT event, COUNT(*) AS n FROM events GROUP BY event ORDER BY n DESC"

hr "10. IN list: filter to West + East sales"
sql --input "$SALES" "SELECT product, region, amount FROM sales WHERE region IN ('West', 'East') ORDER BY amount DESC LIMIT 6"

hr "11. BETWEEN: mid-range sales"
sql --input "$SALES" "SELECT product, amount FROM sales WHERE amount BETWEEN 200 AND 500 ORDER BY amount"

hr "12. Compound aggregate filter: top 5 spenders (salary + sales amount via self-join)"
sql --input "$EMP" --input "$SALES" "SELECT e.name, e.salary + COALESCE(s.amount, 0) AS total FROM employees e LEFT JOIN sales s ON e.id = s.id ORDER BY total DESC LIMIT 5"

hr "Done. Open the cluster dashboard with: $CLI dashboard --addr 127.0.0.1:8081"