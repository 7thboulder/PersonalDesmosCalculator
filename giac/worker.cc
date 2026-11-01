// The Giac worker. Each line on stdin is one Giac command, such as
// integrate(exp(-x^2),x,0,inf). Each answer is one line on stdout:
//
//   \x01ok\t<the result in Giac's syntax>\t<the result in LaTeX>
//   \x01err\t<what went wrong>
//
// The \x01 marks answer lines, since Giac sometimes writes notes of its own.
// Every command runs in the same context, so purge(x) or restart clears
// what earlier commands stored; the app sends restart before each problem.

#include "giac.h"
#include <iostream>
#include <sstream>
#include <string>

using namespace giac;

static std::string one_line(std::string s) {
  for (char &c : s) {
    if (c == '\n' || c == '\r' || c == '\t') c = ' ';
  }
  return s;
}

int main() {
  std::ios::sync_with_stdio(false);
  context ct;
  // Giac's warnings and progress notes go nowhere.
  static std::ostringstream notes;
  logptr(&notes, &ct);
  std::string line;
  while (std::getline(std::cin, line)) {
    if (!line.empty() && line.back() == '\r') line.pop_back();
    if (line.empty()) continue;
    std::string reply;
    try {
      gen g(line, &ct);
      gen r = eval(g, 1, &ct);
      if (r.type == _STRNG && r.subtype == -1) {
        reply = "err\t" + one_line(*r._STRNGptr);
      } else {
        reply = "ok\t" + one_line(r.print(&ct)) + "\t" + one_line(gen2tex(r, &ct));
      }
    } catch (std::runtime_error &e) {
      reply = std::string("err\t") + one_line(e.what());
    } catch (...) {
      reply = "err\tGiac failed";
    }
    notes.str("");
    std::cout << '\x01' << reply << std::endl;
  }
  return 0;
}
