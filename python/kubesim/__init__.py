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
