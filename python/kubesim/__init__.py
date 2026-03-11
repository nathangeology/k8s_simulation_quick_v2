"""KubeSim — Fast Kubernetes cluster simulator."""

from kubesim._native import (
    Simulation, SimResult, StepSimulation, StepObs, batch_run, __version__,
)
from kubesim import analysis

# Adversarial finder — optional (requires hypothesis + pyyaml)
try:
    from kubesim.adversarial import AdversarialFinder, ScenarioSpace, ScoredScenario
except ImportError:
    pass

__all__ = [
    "Simulation", "SimResult", "StepSimulation", "StepObs",
    "batch_run", "analysis",
    "AdversarialFinder", "ScenarioSpace", "ScoredScenario",
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
