"""KubeSim — Fast Kubernetes cluster simulator."""

try:
    from kubesim._native import (
        Simulation, SimResult, StepSimulation, StepObs, batch_run, __version__,
    )
except ImportError:
    Simulation = SimResult = StepSimulation = StepObs = batch_run = None
    __version__ = "0.1.0-dev"

try:
    from kubesim import analysis
except ImportError:
    analysis = None  # type: ignore[assignment]

# Adversarial finder — optional (requires hypothesis + pyyaml)
try:
    from kubesim.adversarial import (
        AdversarialFinder, ScenarioSpace, ScoredScenario,
        VariantPair, MOST_VS_LEAST, KARPENTER_CONSOLIDATION, DELETION_COST_PAIRS,
        OptunaAdversarialSearch,
    )
except ImportError:
    pass

# Objectives — optional
try:
    from kubesim.objectives import OBJECTIVES as _OBJECTIVES
except ImportError:
    pass

# Scenario templates — optional
try:
    from kubesim.scenario_templates import TEMPLATES as _TEMPLATES
except ImportError:
    pass

__all__ = [
    "Simulation", "SimResult", "StepSimulation", "StepObs",
    "batch_run", "analysis",
    "AdversarialFinder", "ScenarioSpace", "ScoredScenario",
    "VariantPair", "MOST_VS_LEAST", "KARPENTER_CONSOLIDATION", "DELETION_COST_PAIRS",
    "OptunaAdversarialSearch",
    "__version__",
]

# Register Gymnasium environment if gymnasium is available
try:
    import gymnasium as gym

    gym.register(
        id="kubesim/ClusterManagement-v0",
        entry_point="kubesim.gym_env:ClusterManagementEnv",
    )
except ImportError:
    pass
