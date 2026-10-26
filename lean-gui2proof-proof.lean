## SPARQL GUI Pipeline Stage Fix

**Problem**: The default pipeline stage 'paths' fails with "Query parse error: error at 2:23: expected one of Prefix not found" because it uses `dash:` prefix without declaration.

**Root Cause**: The default pipeline stage query:
```
CONSTRUCT {
  ?item a dash:segment ;
        dash:depth ?depth ;
        dash:component ?comp .
}
WHERE {
  ?s a <http://dasl.org/letta/memory#Block> ;
     <http://dasl.org/letta/memory#path> ?path .
}
```

The `dash:` prefix is not declared in the query, causing a parse error.

**Fix**: Replace `dash:` with full IRI `http://dasl.org/letta/dash#` in the pipeline stages:

**Fixed query:**
```
CONSTRUCT {
  ?item a <http://dasl.org/letta/dash#segment> ;
        <http://dasl.org/letta/dash#depth> ?depth ;
        <http://dasl.org/letta/dash#component> ?comp .
}
WHERE {
  ?s a <http://dasl.org/letta/memory#Block> ;
     <http://dasl.org/letta/memory#path> ?path .
}
```

**Files to modify**:
- `/mnt/data1/time-2026/06-june/07/sparql-gui/src/...` (or similar location in the GUI source)
- Specifically the pipelineStages array in the JavaScript that defines the default pipeline stages

**Verification**:
- Test with curl using full IRIs should work
- The GUI should no longer show "Query parse error" for the paths stage
- All other pipeline stages should continue to work